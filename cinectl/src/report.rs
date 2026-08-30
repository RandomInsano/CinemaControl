//! HID report wire format, mirroring `firmware/src/hid.rs`'s report
//! descriptors. Feature reports carry a leading Report ID byte (always 0);
//! Input reports read via `HidDevice::read` don't.

use std::fmt;

pub const MAX_BRIGHTNESS: u16 = 1023;

/// 1 Report ID byte + a 16-bit little-endian brightness value.
pub const BRIGHTNESS_FEATURE_REPORT_LEN: usize = 3;
/// A bare 16-bit little-endian brightness value (no Report ID byte).
pub const BRIGHTNESS_INPUT_REPORT_LEN: usize = 2;

pub fn brightness_from_bytes(bytes: [u8; 2]) -> u16 {
    u16::from_le_bytes(bytes)
}

pub fn brightness_feature_report(value: u16) -> [u8; BRIGHTNESS_FEATURE_REPORT_LEN] {
    let [lo, hi] = value.min(MAX_BRIGHTNESS).to_le_bytes();
    [0, lo, hi]
}

pub struct PowerTelemetry {
    pub voltage_mv: u16,
    pub current_ma: i16,
    pub power_mw: u32,
}

/// 1 Report ID byte + voltage (u16 LE) + current (i16 LE) + power (u32 LE).
pub const POWER_FEATURE_REPORT_LEN: usize = 9;
/// The same three fields with no Report ID byte.
pub const POWER_INPUT_REPORT_LEN: usize = 8;

impl PowerTelemetry {
    pub fn from_bytes(bytes: [u8; POWER_INPUT_REPORT_LEN]) -> Self {
        Self {
            voltage_mv: u16::from_le_bytes([bytes[0], bytes[1]]),
            current_ma: i16::from_le_bytes([bytes[2], bytes[3]]),
            power_mw: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        }
    }
}

impl fmt::Display for PowerTelemetry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:.3}V  {:+.3}A  {:.3}W",
            f32::from(self.voltage_mv) / 1000.0,
            f32::from(self.current_ma) / 1000.0,
            self.power_mw as f32 / 1000.0,
        )
    }
}

pub struct ThermalTelemetry {
    pub internal_decic: i16,
    pub external1_decic: i16,
}

/// 1 Report ID byte + internal diode (i16 LE) + external diode 1 (i16 LE).
pub const THERMAL_FEATURE_REPORT_LEN: usize = 5;
/// The same two fields with no Report ID byte.
pub const THERMAL_INPUT_REPORT_LEN: usize = 4;

impl ThermalTelemetry {
    pub fn from_bytes(bytes: [u8; THERMAL_INPUT_REPORT_LEN]) -> Self {
        Self {
            internal_decic: i16::from_le_bytes([bytes[0], bytes[1]]),
            external1_decic: i16::from_le_bytes([bytes[2], bytes[3]]),
        }
    }
}

impl fmt::Display for ThermalTelemetry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "internal {:.1}°C  external1 {:.1}°C",
            f32::from(self.internal_decic) / 10.0,
            f32::from(self.external1_decic) / 10.0,
        )
    }
}
