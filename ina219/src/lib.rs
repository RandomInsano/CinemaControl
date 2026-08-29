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
//! [`Ina219::current_ma`]/[`Ina219::power_mw`] with [`Error::NotCalibrated`]
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
    /// 0 until [`Self::calibrate`] succeeds — see [`Error::NotCalibrated`].
    current_lsb_ma: u32,
}

impl<I2C: I2c> Ina219<I2C> {
    pub fn new(i2c: I2C, address: impl Into<u8>) -> Self {
        Self {
            i2c,
            address: address.into(),
            current_lsb_ma: 0,
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
    /// driver's own `current_lsb_ma` bookkeeping to match, so a stale
    /// calibration can't be used to silently misdecode Current/Power after
    /// the chip has actually forgotten it.
    pub async fn reset(&mut self) -> Result<(), Error<I2C::Error>> {
        self.write_register(Register::Configuration, 1 << 15)
            .await?;
        self.current_lsb_ma = 0;
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

    /// Programs the Calibration register (0x05) with `calibration_raw`, and
    /// records `current_lsb_ma` for decoding [`Self::current_ma`]/
    /// [`Self::power_mw`] afterwards. Both are per-board constants computed
    /// once at design time from the shunt resistor and expected current
    /// range: `calibration_raw = trunc(0.04096 / (Current_LSB_A *
    /// R_SHUNT_ohm))` with bit 0 cleared (datasheet S8.6). Required before
    /// [`Self::current_ma`]/[`Self::power_mw`] will do anything but error;
    /// must be re-run after every [`Self::reset`], since Calibration
    /// doesn't persist across one.
    pub async fn calibrate(
        &mut self,
        current_lsb_ma: u32,
        calibration_raw: u16,
    ) -> Result<(), Error<I2C::Error>> {
        self.write_register(Register::Calibration, calibration_raw)
            .await?;
        self.current_lsb_ma = current_lsb_ma;
        Ok(())
    }

    /// Raw Calibration register readback, e.g. to confirm what
    /// [`Self::calibrate`] actually programmed. Not needed for normal use.
    pub async fn calibration_raw(&mut self) -> Result<u16, Error<I2C::Error>> {
        self.read_register(Register::Calibration).await
    }

    /// `Current_LSB` in mA from the most recent successful
    /// [`Self::calibrate`], or 0 if it hasn't been called (since
    /// [`Self::reset`], if ever).
    pub fn current_lsb_ma(&self) -> u32 {
        self.current_lsb_ma
    }

    /// Current register (0x04). Errors with [`Error::NotCalibrated`] instead
    /// of decoding a real-looking-but-meaningless 0mA if [`Self::calibrate`]
    /// hasn't run yet — see the crate-level doc comment.
    pub async fn current_ma(&mut self) -> Result<i32, Error<I2C::Error>> {
        if self.current_lsb_ma == 0 {
            return Err(Error::NotCalibrated);
        }
        let raw = self.read_register(Register::Current).await? as i16;
        Ok(raw as i32 * self.current_lsb_ma as i32)
    }

    /// Power register (0x03), internally computed by the chip as `(Current x
    /// Bus Voltage) / 5000`, with `Power_LSB = 20 x Current_LSB` (datasheet
    /// S8.6, fixed 20x relationship, not independently configurable) — exact
    /// in mW/mA since both units carry the same 1000x scaling from
    /// watts/amps. Same [`Error::NotCalibrated`] guard as [`Self::current_ma`].
    /// Widened to `u64` for the multiply so a large `current_lsb_ma` can't
    /// silently overflow before landing back in `u32` milliwatts.
    pub async fn power_mw(&mut self) -> Result<u32, Error<I2C::Error>> {
        if self.current_lsb_ma == 0 {
            return Err(Error::NotCalibrated);
        }
        let raw = self.read_register(Register::Power).await?;
        let power_lsb_mw = 20 * self.current_lsb_ma as u64;
        let mw = raw as u64 * power_lsb_mw;
        Ok(mw as u32)
    }
}
