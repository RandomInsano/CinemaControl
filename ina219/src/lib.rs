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
//! [`Ina219::calibrate`] has been called — see [`Ina219::new`]'s doc comment
//! for `current_lsb_ma`/`calibration_raw`, the two board constants that
//! drive both [`Ina219::calibrate`] and how [`Ina219::current_ma`]/
//! [`Ina219::power_mw`] decode their readings.
#![cfg_attr(not(test), no_std)]

pub mod config;
mod error;
pub mod regs;

pub use config::{AdcSetting, BusVoltageRange, Configuration, Gain, Mode};
pub use error::Error;
pub use regs::Register;

use embedded_hal_async::i2c::I2c;

/// Fixed 7-bit SMBus addresses reachable via the A0/A1 strap pins (datasheet
/// S6.1) — variant names are `A1A0`, each half one of the four strap levels
/// the pin supports (Gnd, Vs (VS+), Sda, or Scl bus line). `GndGnd` (`0x40`)
/// is the simplest and most common strap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Address {
    GndGnd = 0x40,
    GndVs = 0x41,
    GndSda = 0x42,
    GndScl = 0x43,
    VsGnd = 0x44,
    VsVs = 0x45,
    VsSda = 0x46,
    VsScl = 0x47,
    SdaGnd = 0x48,
    SdaVs = 0x49,
    SdaSda = 0x4A,
    SdaScl = 0x4B,
    SclGnd = 0x4C,
    SclVs = 0x4D,
    SclSda = 0x4E,
    SclScl = 0x4F,
}

impl From<Address> for u8 {
    fn from(addr: Address) -> u8 {
        addr as u8
    }
}

/// Bus Voltage register (0x02), decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusVoltage {
    /// LSB = 4mV, so this is always an exact multiple of 4 — no precision
    /// lost by using millivolts here, unlike Shunt Voltage's 10µV LSB (see
    /// [`Ina219::shunt_voltage_uv`]).
    pub millivolts: u16,
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

pub struct Ina219<I2C> {
    i2c: I2C,
    address: u8,
    current_lsb_ma: u32,
    calibration_raw: u16,
}

impl<I2C: I2c> Ina219<I2C> {
    /// `current_lsb_ma` and `calibration_raw` are per-board constants,
    /// fixed for the life of the physical device — both computed once at
    /// design time from the shunt resistor and expected current range, and
    /// never anything that changes while the circuit is running (datasheet
    /// S8.6): `calibration_raw = trunc(0.04096 / (Current_LSB_A *
    /// R_SHUNT_ohm))` with bit 0 cleared, and `current_lsb_ma` is that same
    /// `Current_LSB_A` in mA. Taking them here rather than as
    /// [`Self::calibrate`] arguments means a fresh `Ina219` — e.g.
    /// constructed per poll cycle on a bus shared with other devices — can
    /// decode [`Self::current_ma`]/[`Self::power_mw`] correctly without
    /// necessarily being the instance that last wrote Calibration itself.
    pub fn new(
        i2c: I2C,
        address: impl Into<u8>,
        current_lsb_ma: u32,
        calibration_raw: u16,
    ) -> Self {
        Self {
            i2c,
            address: address.into(),
            current_lsb_ma,
            calibration_raw,
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
    /// *not* restore itself automatically (datasheet S8.6). Call
    /// [`Self::calibrate`] again afterwards to reprogram it; `current_lsb_ma`
    /// itself doesn't change (it's a fixed board property, not calibration
    /// state), so [`Self::current_ma`]/[`Self::power_mw`] will keep decoding
    /// with it even though the chip is reporting raw 0x0000 until then.
    pub async fn reset(&mut self) -> Result<(), Error<I2C::Error>> {
        self.write_register(Register::Configuration, 1 << 15).await
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

    /// Shunt Voltage register (0x01) — signed, LSB = 10µV fixed regardless
    /// of PGA gain (PGA only changes the full-scale range before clipping).
    /// Always valid, independent of calibration state.
    ///
    /// Reported in microvolts rather than millivolts: the shunt's own
    /// full-scale range tops out at ±320mV (see [`Gain::full_scale_mv`]), so
    /// rounding to whole millivolts here would throw away most of the
    /// signal's actual resolution.
    pub async fn shunt_voltage_uv(&mut self) -> Result<i32, Error<I2C::Error>> {
        let raw = self.read_register(Register::ShuntVoltage).await? as i16;
        Ok(raw as i32 * 10)
    }

    /// LSB = 4mV, register bits 15:3 (datasheet S8.4). The 13-bit field
    /// maxes out at 8191 * 4 = 32764mV, comfortably inside `u16`.
    pub async fn bus_voltage(&mut self) -> Result<BusVoltage, Error<I2C::Error>> {
        let raw = self.read_register(Register::BusVoltage).await?;
        Ok(BusVoltage {
            millivolts: (raw >> 3) * 4,
            conversion_ready: raw & (1 << 1) != 0,
            overflow: raw & 1 != 0,
        })
    }

    /// Writes `calibration_raw` (given to [`Self::new`]) to the Calibration
    /// register (0x05). Required before [`Self::current_ma`]/
    /// [`Self::power_mw`] will read anything but 0; must be re-run after
    /// every [`Self::reset`], since Calibration doesn't persist across one.
    pub async fn calibrate(&mut self) -> Result<(), Error<I2C::Error>> {
        self.write_register(Register::Calibration, self.calibration_raw)
            .await
    }

    /// Raw Calibration register readback, e.g. to confirm what
    /// [`Self::calibrate`] actually programmed. Not needed for normal use.
    pub async fn calibration_raw(&mut self) -> Result<u16, Error<I2C::Error>> {
        self.read_register(Register::Calibration).await
    }

    /// `Current_LSB` in mA, as given to [`Self::new`].
    pub fn current_lsb_ma(&self) -> u32 {
        self.current_lsb_ma
    }

    /// Current register (0x04). Reads back 0 until [`Self::calibrate`] has
    /// actually been run on the physical device (by this instance or
    /// another one — see [`Self::new`]) — this driver trusts the caller on
    /// that rather than tracking it at runtime. Widened to `i64` for the
    /// multiply, same reasoning as [`Self::power_mw`]: a `current_lsb_ma`
    /// above `i32::MAX` would otherwise get reinterpreted as negative by an
    /// `as i32` cast before the multiply even ran.
    pub async fn current_ma(&mut self) -> Result<i32, Error<I2C::Error>> {
        let raw = self.read_register(Register::Current).await? as i16;
        let ma = raw as i64 * self.current_lsb_ma as i64;
        Ok(ma as i32)
    }

    /// Power register (0x03), internally computed by the chip as `(Current x
    /// Bus Voltage) / 5000`, with `Power_LSB = 20 x Current_LSB` (datasheet
    /// S8.6, fixed 20x relationship, not independently configurable) — exact
    /// in mW/mA since both units carry the same 1000x scaling from
    /// watts/amps. Same calibration caveat as [`Self::current_ma`]. Widened
    /// to `u64` for the multiply so a large `current_lsb_ma` can't silently
    /// overflow before landing back in `u32` milliwatts.
    pub async fn power_mw(&mut self) -> Result<u32, Error<I2C::Error>> {
        let raw = self.read_register(Register::Power).await?;
        let power_lsb_mw = 20 * self.current_lsb_ma as u64;
        let mw = raw as u64 * power_lsb_mw;
        Ok(mw as u32)
    }
}
