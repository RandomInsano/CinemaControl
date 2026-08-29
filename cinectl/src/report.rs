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
/// field layout and units. Voltage/current/power are real INA219 reads,
/// calibrated per `firmware/src/smbus.rs::INA219_CALIBRATION_RAW`; the two
/// temperature fields (Internal Diode, External Diode 1) are real EMC1403
/// reads.
pub struct PsuTelemetry {
    pub voltage_mv: u16,
    /// Signed — the INA219 is bidirectional.
    pub current_ma: i16,
    pub power_mw: u32,
    pub internal_decic: i16,
    pub external1_decic: i16,
}

/// 1 Report ID byte + voltage (u16 LE) + current (i16 LE) + power (u32 LE) +
/// internal diode (i16 LE) + external diode 1 (i16 LE).
pub const PSU_FEATURE_REPORT_LEN: usize = 13;
/// The same five fields with no Report ID byte.
pub const PSU_INPUT_REPORT_LEN: usize = 12;

impl PsuTelemetry {
    pub fn from_bytes(bytes: [u8; PSU_INPUT_REPORT_LEN]) -> Self {
        Self {
            voltage_mv: u16::from_le_bytes([bytes[0], bytes[1]]),
            current_ma: i16::from_le_bytes([bytes[2], bytes[3]]),
            power_mw: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            internal_decic: i16::from_le_bytes([bytes[8], bytes[9]]),
            external1_decic: i16::from_le_bytes([bytes[10], bytes[11]]),
        }
    }
}

impl fmt::Display for PsuTelemetry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:.3} V  {:.3} A  {:.3} W  internal {:.1}°C  external1 {:.1}°C",
            f32::from(self.voltage_mv) / 1000.0,
            f32::from(self.current_ma) / 1000.0,
            self.power_mw as f32 / 1000.0,
            f32::from(self.internal_decic) / 10.0,
            f32::from(self.external1_decic) / 10.0,
        )
    }
}
