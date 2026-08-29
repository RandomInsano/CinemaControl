//! HID report wire format, mirroring `firmware/src/hid.rs`'s report
//! descriptors. Neither interface uses numbered reports, so hidapi's
//! feature-report calls still carry a leading Report ID byte (always 0, per
//! `HidDevice::get_feature_report`/`send_feature_report`'s convention) while
//! Input reports read via `HidDevice::read` don't.

use std::fmt;

/// Mirrors `firmware/src/hid.rs::MAX_BRIGHTNESS`.
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

/// Bundled PSU telemetry, matching `firmware/src/smbus.rs::PsuTelemetry`'s
/// field layout and units. Voltage/current are still placeholder pending a
/// decoded PMBus register map; the two temperature fields (Internal Diode,
/// External Diode 1) are real EMC1403 reads.
pub struct PsuTelemetry {
    pub voltage_mv: u16,
    pub current_ma: u16,
    pub internal_decic: i16,
    pub external1_decic: i16,
}

/// 1 Report ID byte + voltage (u16 LE) + current (u16 LE) + internal diode
/// (i16 LE) + external diode 1 (i16 LE).
pub const PSU_FEATURE_REPORT_LEN: usize = 9;
/// The same four fields with no Report ID byte.
pub const PSU_INPUT_REPORT_LEN: usize = 8;

impl PsuTelemetry {
    pub fn from_bytes(bytes: [u8; PSU_INPUT_REPORT_LEN]) -> Self {
        Self {
            voltage_mv: u16::from_le_bytes([bytes[0], bytes[1]]),
            current_ma: u16::from_le_bytes([bytes[2], bytes[3]]),
            internal_decic: i16::from_le_bytes([bytes[4], bytes[5]]),
            external1_decic: i16::from_le_bytes([bytes[6], bytes[7]]),
        }
    }
}

impl fmt::Display for PsuTelemetry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:.3} V  {:.3} A  internal {:.1}°C  external1 {:.1}°C",
            f32::from(self.voltage_mv) / 1000.0,
            f32::from(self.current_ma) / 1000.0,
            f32::from(self.internal_decic) / 10.0,
            f32::from(self.external1_decic) / 10.0,
        )
    }
}
