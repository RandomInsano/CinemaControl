//! Driver for the TI INA219 zero-drift, bidirectional current/power
//! monitor, transcribed from datasheet SBOS448. Generic over
//! `embedded-hal-async`'s [`I2c`] trait rather than any particular board's
//! bus type, so it drops onto whatever transport a caller already has.
//!
//! Unlike the EMC1403, this part has no Manufacturer/Product/Revision
//! registers to identify itself with — it only exposes six registers total
//! (0x00-0x05, see [`regs::Register`]), so there's no `identify`/`probe`
//! equivalent here. There's also no auto-increment across registers: one
//! pointer write selects exactly one register for however many reads follow,
//! until it's pointed elsewhere.
//!
//! Current and Power (registers 0x04/0x03) read back 0x0000 until
//! [`Ina219::calibrate`] has been called — this driver refuses
//! [`Ina219::current_a`]/[`Ina219::power_w`] with [`Error::NotCalibrated`]
//! rather than silently returning that zero as if it were a real reading.
#![cfg_attr(not(test), no_std)]

pub mod config;
mod error;
pub mod regs;

pub use config::{AdcSetting, BusVoltageRange, Configuration, Gain, Mode};
pub use error::Error;
pub use regs::Register;

use embedded_hal_async::i2c::I2c;

/// Fixed 7-bit SMBus addresses reachable via the A0/A1 strap pins (datasheet
/// S6.1) — named `A1_A0`, each half one of the four strap levels the pin
/// supports (GND, VS+, SDA, or SCL bus line). Both pins grounded (`0x40`) is
/// the simplest and most common strap.
pub mod address {
    pub const GND_GND: u8 = 0x40;
    pub const GND_VS: u8 = 0x41;
    pub const GND_SDA: u8 = 0x42;
    pub const GND_SCL: u8 = 0x43;
    pub const VS_GND: u8 = 0x44;
    pub const VS_VS: u8 = 0x45;
    pub const VS_SDA: u8 = 0x46;
    pub const VS_SCL: u8 = 0x47;
    pub const SDA_GND: u8 = 0x48;
    pub const SDA_VS: u8 = 0x49;
    pub const SDA_SDA: u8 = 0x4A;
    pub const SDA_SCL: u8 = 0x4B;
    pub const SCL_GND: u8 = 0x4C;
    pub const SCL_VS: u8 = 0x4D;
    pub const SCL_SDA: u8 = 0x4E;
    pub const SCL_SCL: u8 = 0x4F;
}

/// Bus Voltage register (0x02), decoded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BusVoltage {
    pub volts: f32,
    /// Set when a conversion has completed. Cleared by reading the Power
    /// register (0x03) or writing Configuration — *not* by reading Bus
    /// Voltage itself. Pollers that want this to actually clear should read
    /// Power every cycle too, not just Bus Voltage.
    pub conversion_ready: bool,
    /// Set when the Power/Current calculation has overflowed the chip's
    /// internal math — usually an under-ranged PGA for the actual load, or a
    /// too-small Calibration value. Treat any reading taken while this is
    /// set as invalid.
    pub overflow: bool,
}

/// LSB = 4mV, register bits 15:3 (datasheet S8.4).
fn decode_bus_voltage(raw: u16) -> BusVoltage {
    BusVoltage {
        volts: (raw >> 3) as f32 * 0.004,
        conversion_ready: raw & (1 << 1) != 0,
        overflow: raw & 1 != 0,
    }
}

/// Step 2 of the datasheet's Calibration procedure (S8.6): the smallest
/// Current_LSB (in amps) that keeps `max_expected_amps` from clipping the
/// signed 16-bit Current register. The datasheet recommends rounding this
/// *up* to a convenient value (1mA, 100uA, ...) before calling
/// [`Ina219::calibrate`] — deliberately left to the caller rather than
/// guessed at here, since "convenient" depends on the units the caller wants
/// Current/Power reported in.
pub fn min_current_lsb_a(max_expected_amps: f32) -> f32 {
    max_expected_amps / 32767.0
}

/// Calibration register formula (datasheet S8.6): `trunc(0.04096 /
/// (Current_LSB * R_SHUNT))`, then bit 0 forced to 0 since calibration
/// values are inherently even (effectively 15 usable bits). Rounds rather
/// than truncating the division itself (widened to `f64` first): a "nice"
/// decimal input like 0.001A/5mOhm isn't exactly representable in `f32`, so
/// truncating can knock an intended-exact 8192.0 down to 8191 purely from
/// that representation error, not from the datasheet's `trunc`. `+ 0.5` then
/// `as u32` rather than `f64::round` — `no_std` has no `round`/`floor`
/// without pulling in `libm`, and inputs here are never negative, so this is
/// exact.
fn calibration_value(current_lsb_a: f32, r_shunt_ohms: f32) -> u16 {
    let raw = (0.04096 / (current_lsb_a as f64 * r_shunt_ohms as f64) + 0.5) as u32;
    (raw & !1) as u16
}

pub struct Ina219<I2C> {
    i2c: I2C,
    address: u8,
    /// 0.0 until [`Self::calibrate`] succeeds — see [`Error::NotCalibrated`].
    current_lsb_a: f32,
}

impl<I2C: I2c> Ina219<I2C> {
    pub fn new(i2c: I2C, address: u8) -> Self {
        Self {
            i2c,
            address,
            current_lsb_a: 0.0,
        }
    }

    /// Gives back the underlying bus, e.g. to reuse it for another device
    /// sharing the same SMBus.
    pub fn release(self) -> I2C {
        self.i2c
    }

    pub async fn read_register(&mut self, reg: impl Into<u8>) -> Result<u16, Error<I2C::Error>> {
        let mut buf = [0u8; 2];
        self.i2c
            .write_read(self.address, &[reg.into()], &mut buf)
            .await?;
        Ok(u16::from_be_bytes(buf))
    }

    pub async fn write_register(
        &mut self,
        reg: impl Into<u8>,
        value: u16,
    ) -> Result<(), Error<I2C::Error>> {
        let [hi, lo] = value.to_be_bytes();
        self.i2c.write(self.address, &[reg.into(), hi, lo]).await?;
        Ok(())
    }

    /// Sets RST (bit 15), forcing every register back to its power-on
    /// default — including Calibration, which resets to 0x0000 and does
    /// *not* restore itself automatically (datasheet S8.6). Clears this
    /// driver's own `current_lsb_a` bookkeeping to match, so a stale
    /// calibration can't be used to silently misdecode Current/Power after
    /// the chip has actually forgotten it.
    pub async fn reset(&mut self) -> Result<(), Error<I2C::Error>> {
        self.write_register(Register::Configuration, 1 << 15)
            .await?;
        self.current_lsb_a = 0.0;
        Ok(())
    }

    pub async fn configuration(&mut self) -> Result<Configuration, Error<I2C::Error>> {
        Ok(Configuration::from_raw(
            self.read_register(Register::Configuration).await?,
        ))
    }

    pub async fn set_configuration(&mut self, cfg: Configuration) -> Result<(), Error<I2C::Error>> {
        self.write_register(Register::Configuration, cfg.to_raw())
            .await
    }

    /// Shunt Voltage register (0x01) — signed, LSB = 10uV fixed regardless
    /// of PGA gain (PGA only changes the full-scale range before clipping).
    /// Always valid, independent of calibration state.
    pub async fn shunt_voltage_v(&mut self) -> Result<f32, Error<I2C::Error>> {
        let raw = self.read_register(Register::ShuntVoltage).await? as i16;
        Ok(raw as f32 * 10e-6)
    }

    pub async fn bus_voltage(&mut self) -> Result<BusVoltage, Error<I2C::Error>> {
        Ok(decode_bus_voltage(
            self.read_register(Register::BusVoltage).await?,
        ))
    }

    /// Programs the Calibration register (0x05) for `current_lsb_a` amps/bit
    /// against a shunt resistor of `r_shunt_ohms` ohms — see
    /// [`min_current_lsb_a`] for picking `current_lsb_a`. Required before
    /// [`Self::current_a`]/[`Self::power_w`] will do anything but error;
    /// must be re-run after every [`Self::reset`], since Calibration doesn't
    /// persist across one.
    pub async fn calibrate(
        &mut self,
        current_lsb_a: f32,
        r_shunt_ohms: f32,
    ) -> Result<(), Error<I2C::Error>> {
        let cal = calibration_value(current_lsb_a, r_shunt_ohms);
        self.write_register(Register::Calibration, cal).await?;
        self.current_lsb_a = current_lsb_a;
        Ok(())
    }

    /// Raw Calibration register readback, e.g. to confirm what
    /// [`Self::calibrate`] actually programmed. Not needed for normal use.
    pub async fn calibration_raw(&mut self) -> Result<u16, Error<I2C::Error>> {
        self.read_register(Register::Calibration).await
    }

    /// `Current_LSB` from the most recent successful [`Self::calibrate`], or
    /// 0.0 if it hasn't been called (since [`Self::reset`], if ever).
    pub fn current_lsb_a(&self) -> f32 {
        self.current_lsb_a
    }

    /// `Power_LSB = 20 x Current_LSB` (datasheet S8.6, fixed 20x
    /// relationship, not independently configurable).
    pub fn power_lsb_w(&self) -> f32 {
        20.0 * self.current_lsb_a
    }

    /// Current register (0x04). Errors with [`Error::NotCalibrated`] instead
    /// of decoding a real-looking-but-meaningless 0.0A if [`Self::calibrate`]
    /// hasn't run yet — see the crate-level doc comment.
    pub async fn current_a(&mut self) -> Result<f32, Error<I2C::Error>> {
        if self.current_lsb_a == 0.0 {
            return Err(Error::NotCalibrated);
        }
        let raw = self.read_register(Register::Current).await? as i16;
        Ok(raw as f32 * self.current_lsb_a)
    }

    /// Power register (0x03), internally computed by the chip as `(Current x
    /// Bus Voltage) / 5000`. Same [`Error::NotCalibrated`] guard as
    /// [`Self::current_a`].
    pub async fn power_w(&mut self) -> Result<f32, Error<I2C::Error>> {
        if self.current_lsb_a == 0.0 {
            return Err(Error::NotCalibrated);
        }
        let raw = self.read_register(Register::Power).await?;
        Ok(raw as f32 * self.power_lsb_w())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_bus_voltage_default_state() {
        // raw 8 in the 13-bit field -> 8 * 4mV = 32mV, CNVR set, OVF clear.
        let raw = (8u16 << 3) | 0b10;
        let v = decode_bus_voltage(raw);
        assert_eq!(v.volts, 0.032);
        assert!(v.conversion_ready);
        assert!(!v.overflow);
    }

    #[test]
    fn bus_voltage_overflow_bit_is_independent_of_value() {
        let raw = (100u16 << 3) | 0b01;
        let v = decode_bus_voltage(raw);
        assert!(v.overflow);
        assert!(!v.conversion_ready);
    }

    #[test]
    fn min_current_lsb_matches_full_scale_range() {
        // 20A over signed 16-bit -> ~610uA/bit.
        let lsb = min_current_lsb_a(20.0);
        assert!((lsb - 0.00061).abs() < 0.00001);
    }

    #[test]
    fn calibration_value_matches_worked_example() {
        // R_SHUNT=5mOhm, Current_LSB=1mA -> Cal=8192 (0x2000), per the
        // datasheet-derived worked example this driver was built against.
        assert_eq!(calibration_value(0.001, 0.005), 8192);
    }

    #[test]
    fn calibration_value_bit_zero_is_always_clear() {
        // Pick inputs that would otherwise truncate to an odd value.
        let cal = calibration_value(0.0009999, 0.005);
        assert_eq!(cal & 1, 0);
    }
}
