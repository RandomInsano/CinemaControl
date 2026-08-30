//! Driver for the Microchip EMC1403/EMC1404 SMBus temperature sensor
//! family, transcribed from datasheet DS20005272A. Generic over
//! `embedded-hal-async`'s [`I2c`] trait rather than any particular board's
//! bus type, so it drops onto whatever transport a caller already has.
//!
//! Register-level accessors ([`Emc1403::read_register`]/
//! [`Emc1403::write_register`]) are exposed alongside the typed ones so
//! registers this driver doesn't model directly (Beta Configuration,
//! Ideality Factor, Filter Control, Scratchpad — see [`regs`]) are still
//! reachable without going through a second abstraction.
#![cfg_attr(not(test), no_std)]

mod channel;
mod error;
pub mod flags;
pub mod regs;

pub use channel::Channel;
pub use error::Error;
pub use flags::{ChannelMask, Configuration, DiodeFault, LimitStatus, Status};
pub use regs::Register;

use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;
use num_enum::{IntoPrimitive, TryFromPrimitive};

/// Fixed 7-bit SMBus addresses (datasheet S1) — the part has no address
/// pin, so which of these four applies is set at the factory by order
/// code.
pub mod address {
    pub const EMC1403_1_EMC1404_1: u8 = 0x4C;
    pub const EMC1403_2_EMC1404_2: u8 = 0x4D;
    pub const EMC1403_3_EMC1404_3: u8 = 0x18;
    pub const EMC1403_4_EMC1404_4: u8 = 0x29;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Product {
    /// Internal diode + 2 external diodes.
    Emc1403,
    /// Internal diode + 3 external diodes.
    Emc1404,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceId {
    pub product: Product,
    pub revision: u8,
}

/// CONV\[3:0\] field of the Conversion Rate register (0x04/0x0A, mirrored).
///
/// Variant order doubles as the raw encoding (`PerSec1_16` = 0 .. `PerSec64`
/// = 0xA), so [`IntoPrimitive`] and [`TryFromPrimitive`] derive `to_raw`/
/// `from_raw` from the discriminants instead of a hand-mirrored match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
#[repr(u8)]
pub enum ConversionRate {
    PerSec1_16,
    PerSec1_8,
    PerSec1_4,
    PerSec1_2,
    PerSec1,
    PerSec2,
    PerSec4,
    PerSec8,
    PerSec16,
    PerSec32,
    PerSec64,
}

impl ConversionRate {
    fn from_raw(raw: u8) -> Self {
        // 0xB..=0xF are undefined and fall back to 1/sec (datasheet S7).
        Self::try_from(raw & 0x0F).unwrap_or(Self::PerSec1)
    }

    fn to_raw(self) -> u8 {
        self.into()
    }

    /// Time between conversions at this rate, in microseconds — the plain
    /// reciprocal of the variant name (`PerSec4` -> 4 conversions/sec ->
    /// 250,000us between them), not a separate datasheet-measured figure.
    pub const fn period_us(self) -> u32 {
        match self {
            Self::PerSec1_16 => 16_000_000,
            Self::PerSec1_8 => 8_000_000,
            Self::PerSec1_4 => 4_000_000,
            Self::PerSec1_2 => 2_000_000,
            Self::PerSec1 => 1_000_000,
            Self::PerSec2 => 500_000,
            Self::PerSec4 => 250_000,
            Self::PerSec8 => 125_000,
            Self::PerSec16 => 62_500,
            Self::PerSec32 => 31_250,
            Self::PerSec64 => 15_625,
        }
    }
}

/// CALRT[2:0]/CTHRM[2:0] fields of the Consecutive ALERT register (0x22)
/// only define four of their eight possible bit patterns (datasheet S9) —
/// not a plain binary count, so this exists to keep `0b011` meaning "3"
/// from having to be memorized at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsecutiveCount {
    One,
    Two,
    Three,
    Four,
}

impl ConsecutiveCount {
    fn from_field(field: u8) -> Self {
        match field & 0b111 {
            0b001 => Self::Two,
            0b011 => Self::Three,
            0b111 => Self::Four,
            // 0b000 plus every undefined pattern reads back as "1".
            _ => Self::One,
        }
    }

    fn to_field(self) -> u8 {
        match self {
            Self::One => 0b000,
            Self::Two => 0b001,
            Self::Three => 0b011,
            Self::Four => 0b111,
        }
    }
}

/// Consecutive ALERT register (0x22), decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsecutiveAlertConfig {
    /// Bit 7. If set, SMCLK held low >30ms resets the device's SMBus
    /// protocol state. Disabled (false) by default.
    pub smbus_timeout_enabled: bool,
    /// CTHRM\[2:0\] — consecutive out-of-limit measurements required before
    /// THERM asserts.
    pub therm_count: ConsecutiveCount,
    /// CALRT\[2:0\] — consecutive out-of-limit/fault measurements required
    /// before ALERT asserts.
    pub alert_count: ConsecutiveCount,
}

impl ConsecutiveAlertConfig {
    fn from_raw(raw: u8) -> Self {
        Self {
            smbus_timeout_enabled: raw & (1 << 7) != 0,
            therm_count: ConsecutiveCount::from_field((raw >> 4) & 0b111),
            alert_count: ConsecutiveCount::from_field((raw >> 1) & 0b111),
        }
    }

    fn to_raw(self) -> u8 {
        ((self.smbus_timeout_enabled as u8) << 7)
            | (self.therm_count.to_field() << 4)
            | (self.alert_count.to_field() << 1)
    }
}

/// High byte = integer part, low byte's top 3 bits = fractional part in
/// 0.125C steps (datasheet S4).
fn decode_temp_c(high: u8, low: u8, range_extended: bool) -> f32 {
    let whole = if range_extended {
        high as f32 - 64.0
    } else {
        high as f32
    };
    let frac = ((low >> 5) & 0b111) as f32 * 0.125;
    whole + frac
}

pub struct Emc1403<I2C> {
    i2c: I2C,
    address: u8,
}

impl<I2C: I2c> Emc1403<I2C> {
    pub fn new(i2c: I2C, address: u8) -> Self {
        Self { i2c, address }
    }

    /// Gives back the underlying bus, e.g. to reuse it for another device
    /// sharing the same SMBus.
    pub fn release(self) -> I2C {
        self.i2c
    }

    pub async fn read_register(&mut self, reg: impl Into<u8>) -> Result<u8, Error<I2C::Error>> {
        let mut buf = [0u8; 1];
        self.i2c
            .write_read(self.address, &[reg.into()], &mut buf)
            .await?;
        Ok(buf[0])
    }

    pub async fn write_register(
        &mut self,
        reg: impl Into<u8>,
        value: u8,
    ) -> Result<(), Error<I2C::Error>> {
        self.i2c.write(self.address, &[reg.into(), value]).await?;
        Ok(())
    }

    /// Reads Manufacturer ID (0xFE), Product ID (0xFD), and Revision (0xFF),
    /// and fails with [`Error::UnexpectedDevice`] if they don't match a
    /// Microchip EMC1403/EMC1404 — no retry, unlike [`Self::probe`].
    pub async fn identify(&mut self) -> Result<DeviceId, Error<I2C::Error>> {
        let manufacturer = self.read_register(Register::ManufacturerId).await?;
        let product_raw = self.read_register(Register::ProductId).await?;
        let revision = self.read_register(Register::Revision).await?;

        let product = match product_raw {
            regs::PRODUCT_ID_EMC1403 => Product::Emc1403,
            regs::PRODUCT_ID_EMC1404 => Product::Emc1404,
            _ => {
                return Err(Error::UnexpectedDevice {
                    product: product_raw,
                    manufacturer,
                });
            }
        };
        if manufacturer != regs::MANUFACTURER_ID_VALUE {
            return Err(Error::UnexpectedDevice {
                product: product_raw,
                manufacturer,
            });
        }
        Ok(DeviceId { product, revision })
    }

    /// Like [`Self::identify`], but retries on bus errors for the first
    /// ~15ms after power-up, since the device may not respond to SMBus
    /// communication until then (datasheet S2). Use this at startup; use
    /// `identify` directly once the device is known to already be up.
    pub async fn probe<D: DelayNs>(
        &mut self,
        delay: &mut D,
    ) -> Result<DeviceId, Error<I2C::Error>> {
        const RETRY_INTERVAL_MS: u32 = 2;
        const MAX_RETRIES: u32 = 10; // ~20ms, comfortably past the 15ms window

        let mut attempt = 0;
        loop {
            match self.identify().await {
                Err(Error::Bus(_)) if attempt < MAX_RETRIES => {
                    attempt += 1;
                    delay.delay_ms(RETRY_INTERVAL_MS).await;
                }
                result => return result,
            }
        }
    }

    async fn range_is_extended(&mut self) -> Result<bool, Error<I2C::Error>> {
        Ok(self.configuration().await?.contains(Configuration::RANGE))
    }

    /// Reads one channel's temperature in degrees C.
    ///
    /// High byte MUST be read before the low byte on every call: reading
    /// the high byte latches the low byte's shadow register (datasheet
    /// S4.1, "Data Read Interlock"). Reading the low byte on its own
    /// returns a stale value from some earlier, unrelated read.
    ///
    /// Re-reads Configuration on every call to check the RANGE bit; if
    /// you're polling at high rate and RANGE is fixed at init, cache it
    /// instead of paying for that extra transaction each time.
    pub async fn read_temp_c(&mut self, ch: Channel) -> Result<f32, Error<I2C::Error>> {
        let (hi_reg, lo_reg) = ch.temp_regs();
        let hi = self.read_register(hi_reg).await?; // read first, always
        let lo = self.read_register(lo_reg).await?; // now-latched shadow value
        let extended = self.range_is_extended().await?;
        Ok(decode_temp_c(hi, lo, extended))
    }

    pub async fn status(&mut self) -> Result<Status, Error<I2C::Error>> {
        Ok(Status::from_bits_truncate(
            self.read_register(Register::Status).await?,
        ))
    }

    /// Read-to-clear. Also clears the HIGH bit in [`Self::status`].
    pub async fn take_high_limit_status(&mut self) -> Result<LimitStatus, Error<I2C::Error>> {
        Ok(LimitStatus::from_bits_truncate(
            self.read_register(Register::HighLimitStatus).await?,
        ))
    }

    /// Read-to-clear. Also clears the LOW bit in [`Self::status`].
    pub async fn take_low_limit_status(&mut self) -> Result<LimitStatus, Error<I2C::Error>> {
        Ok(LimitStatus::from_bits_truncate(
            self.read_register(Register::LowLimitStatus).await?,
        ))
    }

    /// NOT read-to-clear, unlike the other three status registers here —
    /// this one self-clears only once temperature drops below (THERM
    /// limit - hysteresis). This is a peek, not a consume; polling it
    /// repeatedly is safe and won't mask a real THERM condition.
    pub async fn peek_therm_status(&mut self) -> Result<LimitStatus, Error<I2C::Error>> {
        Ok(LimitStatus::from_bits_truncate(
            self.read_register(Register::ThermLimitStatus).await?,
        ))
    }

    /// Read-to-clear. Also clears the FAULT bit in [`Self::status`].
    ///
    /// An external channel reading exactly 0.0C is worth cross-checking
    /// against this before trusting it — an open DP/DN or a short to VDD
    /// reports as a fault with the channel's temperature pinned at
    /// 0x00/0x00, and that's indistinguishable from a genuine 0C reading
    /// without this register (datasheet S5.4).
    pub async fn take_diode_fault(&mut self) -> Result<DiodeFault, Error<I2C::Error>> {
        Ok(DiodeFault::from_bits_truncate(
            self.read_register(Register::ExternalDiodeFault).await?,
        ))
    }

    pub async fn configuration(&mut self) -> Result<Configuration, Error<I2C::Error>> {
        Ok(Configuration::from_bits_truncate(
            self.read_register(Register::Configuration).await?,
        ))
    }

    pub async fn set_configuration(&mut self, cfg: Configuration) -> Result<(), Error<I2C::Error>> {
        self.write_register(Register::Configuration, cfg.bits()).await
    }

    async fn encode_limit(&mut self, whole_degrees_c: i16) -> Result<u8, Error<I2C::Error>> {
        let raw = if self.range_is_extended().await? {
            whole_degrees_c + 64
        } else {
            whole_degrees_c
        };
        Ok(raw as u8)
    }

    async fn decode_limit(&mut self, raw: u8) -> Result<i16, Error<I2C::Error>> {
        let raw = raw as i16;
        Ok(if self.range_is_extended().await? {
            raw - 64
        } else {
            raw
        })
    }

    /// Programs `ch`'s whole-degree High Limit, in whatever data format
    /// (RANGE bit) is currently selected — see [`Self::configuration`].
    /// Has no effect until the next conversion if the device is currently
    /// in Standby (datasheet S8).
    pub async fn set_high_limit_c(
        &mut self,
        ch: Channel,
        whole_degrees_c: i16,
    ) -> Result<(), Error<I2C::Error>> {
        let raw = self.encode_limit(whole_degrees_c).await?;
        self.write_register(ch.high_limit_reg(), raw).await
    }

    pub async fn high_limit_c(&mut self, ch: Channel) -> Result<i16, Error<I2C::Error>> {
        let raw = self.read_register(ch.high_limit_reg()).await?;
        self.decode_limit(raw).await
    }

    pub async fn set_low_limit_c(
        &mut self,
        ch: Channel,
        whole_degrees_c: i16,
    ) -> Result<(), Error<I2C::Error>> {
        let raw = self.encode_limit(whole_degrees_c).await?;
        self.write_register(ch.low_limit_reg(), raw).await
    }

    pub async fn low_limit_c(&mut self, ch: Channel) -> Result<i16, Error<I2C::Error>> {
        let raw = self.read_register(ch.low_limit_reg()).await?;
        self.decode_limit(raw).await
    }

    pub async fn set_therm_limit_c(
        &mut self,
        ch: Channel,
        whole_degrees_c: i16,
    ) -> Result<(), Error<I2C::Error>> {
        let raw = self.encode_limit(whole_degrees_c).await?;
        self.write_register(ch.therm_limit_reg(), raw).await
    }

    pub async fn therm_limit_c(&mut self, ch: Channel) -> Result<i16, Error<I2C::Error>> {
        let raw = self.read_register(ch.therm_limit_reg()).await?;
        self.decode_limit(raw).await
    }

    /// A plain degree delta shared by every THERM limit, not itself
    /// RANGE-encoded (datasheet S8).
    pub async fn set_therm_hysteresis_c(&mut self, degrees_c: u8) -> Result<(), Error<I2C::Error>> {
        self.write_register(Register::ThermHysteresis, degrees_c).await
    }

    pub async fn therm_hysteresis_c(&mut self) -> Result<u8, Error<I2C::Error>> {
        self.read_register(Register::ThermHysteresis).await
    }

    pub async fn conversion_rate(&mut self) -> Result<ConversionRate, Error<I2C::Error>> {
        Ok(ConversionRate::from_raw(
            self.read_register(Register::ConversionRate).await?,
        ))
    }

    pub async fn set_conversion_rate(
        &mut self,
        rate: ConversionRate,
    ) -> Result<(), Error<I2C::Error>> {
        self.write_register(Register::ConversionRate, rate.to_raw())
            .await
    }

    pub async fn channel_mask(&mut self) -> Result<ChannelMask, Error<I2C::Error>> {
        Ok(ChannelMask::from_bits_truncate(
            self.read_register(Register::ChannelMask).await?,
        ))
    }

    pub async fn set_channel_mask(&mut self, mask: ChannelMask) -> Result<(), Error<I2C::Error>> {
        self.write_register(Register::ChannelMask, mask.bits()).await
    }

    pub async fn consecutive_alert_config(
        &mut self,
    ) -> Result<ConsecutiveAlertConfig, Error<I2C::Error>> {
        Ok(ConsecutiveAlertConfig::from_raw(
            self.read_register(Register::ConsecutiveAlert).await?,
        ))
    }

    pub async fn set_consecutive_alert_config(
        &mut self,
        cfg: ConsecutiveAlertConfig,
    ) -> Result<(), Error<I2C::Error>> {
        self.write_register(Register::ConsecutiveAlert, cfg.to_raw())
            .await
    }

    /// Triggers a single conversion across all channels while in Standby
    /// (`Configuration::STANDBY` set) and BUSY clear. No effect in Active
    /// mode. The register always reads back 0x00 — data lands in the usual
    /// temperature registers, not here.
    pub async fn one_shot(&mut self) -> Result<(), Error<I2C::Error>> {
        self.write_register(Register::OneShot, 0x00).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_standard_range() {
        // 0.375C fractional = 0b011 in the top 3 bits of the low byte.
        assert_eq!(decode_temp_c(42, 0b011_00000, false), 42.375);
    }

    #[test]
    fn decodes_extended_range_below_zero() {
        assert_eq!(decode_temp_c(0, 0, true), -64.0);
    }

    #[test]
    fn conversion_rate_round_trips() {
        for raw in 0x0..=0xA {
            assert_eq!(ConversionRate::from_raw(raw).to_raw(), raw);
        }
    }

    #[test]
    fn undefined_conversion_rate_falls_back_to_1_per_sec() {
        assert_eq!(ConversionRate::from_raw(0xF), ConversionRate::PerSec1);
    }

    #[test]
    fn period_matches_named_rate() {
        assert_eq!(ConversionRate::PerSec1.period_us(), 1_000_000);
        assert_eq!(ConversionRate::PerSec4.period_us(), 250_000);
        assert_eq!(ConversionRate::PerSec1_16.period_us(), 16_000_000);
        assert_eq!(ConversionRate::PerSec64.period_us(), 15_625);
    }

    #[test]
    fn consecutive_count_round_trips_defined_codes() {
        for field in [0b000u8, 0b001, 0b011, 0b111] {
            let count = ConsecutiveCount::from_field(field);
            assert_eq!(count.to_field(), field);
        }
    }

    #[test]
    fn consecutive_alert_config_round_trips() {
        let cfg = ConsecutiveAlertConfig {
            smbus_timeout_enabled: true,
            therm_count: ConsecutiveCount::Four,
            alert_count: ConsecutiveCount::Three,
        };
        assert_eq!(ConsecutiveAlertConfig::from_raw(cfg.to_raw()), cfg);
    }

    #[test]
    fn default_consecutive_alert_matches_datasheet_reset_value() {
        // 0x70 = TIMEOUT=0, CTHRM=111 (4), CALRT=000 (1) — datasheet S9.
        let cfg = ConsecutiveAlertConfig::from_raw(0x70);
        assert!(!cfg.smbus_timeout_enabled);
        assert_eq!(cfg.therm_count, ConsecutiveCount::Four);
        assert_eq!(cfg.alert_count, ConsecutiveCount::One);
    }

    #[test]
    fn limit_status_bit_matches_channel_mask_bit() {
        for ch in [
            Channel::Internal,
            Channel::External1,
            Channel::External2,
            Channel::External3,
        ] {
            assert_eq!(
                LimitStatus::for_channel(ch).bits(),
                ChannelMask::for_channel(ch).bits()
            );
        }
    }

    #[test]
    fn diode_fault_has_no_bit_for_internal_channel() {
        assert!(DiodeFault::for_channel(Channel::Internal).is_empty());
    }
}
