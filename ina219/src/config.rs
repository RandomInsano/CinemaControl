//! Configuration register (0x00) fields, datasheet S8.1 (TI SBOS448).

/// BRNG, bit 13.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusVoltageRange {
    V16,
    V32,
}

impl BusVoltageRange {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::V32 } else { Self::V16 }
    }

    fn to_bit(self) -> bool {
        matches!(self, Self::V32)
    }
}

/// PG\[1:0\], bits 12:11 — shunt voltage full-scale range. The LSB of the
/// Shunt Voltage register is always 10uV regardless of this setting; PGA
/// only changes how large a shunt voltage can be represented before
/// clipping (datasheet S7.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gain {
    Div1,
    Div2,
    Div4,
    Div8,
}

impl Gain {
    fn from_raw(raw: u16) -> Self {
        match raw & 0b11 {
            0b00 => Self::Div1,
            0b01 => Self::Div2,
            0b10 => Self::Div4,
            _ => Self::Div8,
        }
    }

    fn to_raw(self) -> u16 {
        match self {
            Self::Div1 => 0b00,
            Self::Div2 => 0b01,
            Self::Div4 => 0b10,
            Self::Div8 => 0b11,
        }
    }

    /// Shunt voltage full-scale range in millivolts before clipping.
    pub fn full_scale_mv(self) -> u16 {
        match self {
            Self::Div1 => 40,
            Self::Div2 => 80,
            Self::Div4 => 160,
            Self::Div8 => 320,
        }
    }
}

/// Shared BADC\[3:0\]/SADC\[3:0\] encoding (bits 10:7 and 6:3 respectively) —
/// the top bit of the 4-bit field switches between single-shot resolution
/// select (`0xxx`) and averaging (`1xxx`, always 12-bit per sample). Only
/// the eleven bit patterns the datasheet actually names are modeled; the
/// rest (`0100`-`0111`, and `1000`'s redundant "12-bit, 1 sample" encoding)
/// fall back to [`Self::Bits12`] on read, matching its own conversion time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdcSetting {
    Bits9,
    Bits10,
    Bits11,
    Bits12,
    Average2,
    Average4,
    Average8,
    Average16,
    Average32,
    Average64,
    Average128,
}

impl AdcSetting {
    fn from_raw(raw: u16) -> Self {
        match raw & 0b1111 {
            0b0000 => Self::Bits9,
            0b0001 => Self::Bits10,
            0b0010 => Self::Bits11,
            0b1001 => Self::Average2,
            0b1010 => Self::Average4,
            0b1011 => Self::Average8,
            0b1100 => Self::Average16,
            0b1101 => Self::Average32,
            0b1110 => Self::Average64,
            0b1111 => Self::Average128,
            _ => Self::Bits12,
        }
    }

    fn to_raw(self) -> u16 {
        match self {
            Self::Bits9 => 0b0000,
            Self::Bits10 => 0b0001,
            Self::Bits11 => 0b0010,
            Self::Bits12 => 0b0011,
            Self::Average2 => 0b1001,
            Self::Average4 => 0b1010,
            Self::Average8 => 0b1011,
            Self::Average16 => 0b1100,
            Self::Average32 => 0b1101,
            Self::Average64 => 0b1110,
            Self::Average128 => 0b1111,
        }
    }
}

/// MODE\[2:0\], bits 2:0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    PowerDown,
    ShuntTriggered,
    BusTriggered,
    ShuntBusTriggered,
    AdcOff,
    ShuntContinuous,
    BusContinuous,
    ShuntBusContinuous,
}

impl Mode {
    fn from_raw(raw: u16) -> Self {
        match raw & 0b111 {
            0b000 => Self::PowerDown,
            0b001 => Self::ShuntTriggered,
            0b010 => Self::BusTriggered,
            0b011 => Self::ShuntBusTriggered,
            0b100 => Self::AdcOff,
            0b101 => Self::ShuntContinuous,
            0b110 => Self::BusContinuous,
            _ => Self::ShuntBusContinuous,
        }
    }

    fn to_raw(self) -> u16 {
        match self {
            Self::PowerDown => 0b000,
            Self::ShuntTriggered => 0b001,
            Self::BusTriggered => 0b010,
            Self::ShuntBusTriggered => 0b011,
            Self::AdcOff => 0b100,
            Self::ShuntContinuous => 0b101,
            Self::BusContinuous => 0b110,
            Self::ShuntBusContinuous => 0b111,
        }
    }

    /// In triggered modes, a new conversion only starts when Configuration
    /// is written with one of the triggered variants again — reading old
    /// data repeatedly without re-triggering just returns the same stale
    /// conversion (datasheet S8.3).
    pub fn is_triggered(self) -> bool {
        matches!(
            self,
            Self::ShuntTriggered | Self::BusTriggered | Self::ShuntBusTriggered
        )
    }
}

/// Configuration register (0x00), decoded. Excludes RST (bit 15, write-only
/// and self-clearing — see [`crate::Ina219::reset`]) and the reserved bit
/// 14, which always reads 0 and is ignored on write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Configuration {
    pub bus_voltage_range: BusVoltageRange,
    pub gain: Gain,
    pub bus_adc: AdcSetting,
    pub shunt_adc: AdcSetting,
    pub mode: Mode,
}

/// Power-on reset value is 0x399F: 32V range, /8 PGA, 12-bit continuous
/// conversion on both ADCs — the most permissive, highest-range continuous
/// mode, matching [`Self::from_raw`]`(0x399F)`.
impl Default for Configuration {
    fn default() -> Self {
        Self {
            bus_voltage_range: BusVoltageRange::V32,
            gain: Gain::Div8,
            bus_adc: AdcSetting::Bits12,
            shunt_adc: AdcSetting::Bits12,
            mode: Mode::ShuntBusContinuous,
        }
    }
}

impl Configuration {
    pub fn from_raw(raw: u16) -> Self {
        Self {
            bus_voltage_range: BusVoltageRange::from_bit(raw & (1 << 13) != 0),
            gain: Gain::from_raw(raw >> 11),
            bus_adc: AdcSetting::from_raw(raw >> 7),
            shunt_adc: AdcSetting::from_raw(raw >> 3),
            mode: Mode::from_raw(raw),
        }
    }

    pub fn to_raw(self) -> u16 {
        ((self.bus_voltage_range.to_bit() as u16) << 13)
            | (self.gain.to_raw() << 11)
            | (self.bus_adc.to_raw() << 7)
            | (self.shunt_adc.to_raw() << 3)
            | self.mode.to_raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_power_on_reset_value() {
        assert_eq!(Configuration::default().to_raw(), 0x399F);
        assert_eq!(Configuration::from_raw(0x399F), Configuration::default());
    }

    #[test]
    fn adc_setting_round_trips_defined_codes() {
        for raw in [
            0b0000u16, 0b0001, 0b0010, 0b0011, 0b1001, 0b1010, 0b1011, 0b1100, 0b1101, 0b1110,
            0b1111,
        ] {
            assert_eq!(AdcSetting::from_raw(raw).to_raw(), raw);
        }
    }

    #[test]
    fn undefined_adc_codes_fall_back_to_12_bit() {
        for raw in [0b0100u16, 0b0101, 0b0110, 0b0111, 0b1000] {
            assert_eq!(AdcSetting::from_raw(raw), AdcSetting::Bits12);
        }
    }

    #[test]
    fn mode_round_trips_every_code() {
        for raw in 0b000u16..=0b111 {
            assert_eq!(Mode::from_raw(raw).to_raw(), raw);
        }
    }

    #[test]
    fn triggered_modes_are_identified() {
        assert!(Mode::ShuntTriggered.is_triggered());
        assert!(Mode::BusTriggered.is_triggered());
        assert!(Mode::ShuntBusTriggered.is_triggered());
        assert!(!Mode::ShuntBusContinuous.is_triggered());
        assert!(!Mode::AdcOff.is_triggered());
    }

    #[test]
    fn configuration_round_trips_arbitrary_value() {
        let cfg = Configuration {
            bus_voltage_range: BusVoltageRange::V16,
            gain: Gain::Div2,
            bus_adc: AdcSetting::Average32,
            shunt_adc: AdcSetting::Bits10,
            mode: Mode::ShuntContinuous,
        };
        assert_eq!(Configuration::from_raw(cfg.to_raw()), cfg);
    }
}
