#![cfg_attr(not(test), no_std)]

use core::fmt;

mod hid_tools;

pub use hid_tools::{Report, ToLeBytes};

// pid.codes shared testing VID:PID.
pub const VENDOR_ID: u16 = 0x1209;
pub const PRODUCT_ID: u16 = 0xCC02;

pub const MAX_BRIGHTNESS: u16 = 1023;

pub const BRIGHTNESS_REPORT_LEN: usize = 2;
pub const POWER_REPORT_LEN: usize = 8;
pub const THERMAL_REPORT_LEN: usize = 4;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct PowerTelemetry {
    pub voltage_mv: u16,
    /// Sign depends on IN+/IN− wiring — see datasheet.
    pub current_ma: i16,
    pub power_mw: u32,
}

impl PowerTelemetry {
    pub fn to_bytes(self) -> [u8; POWER_REPORT_LEN] {
        let mut buf = [0u8; POWER_REPORT_LEN];
        Report::new(&mut buf)
            .field(self.voltage_mv)
            .field(self.current_ma)
            .field(self.power_mw);
        buf
    }

    pub fn from_bytes(bytes: [u8; POWER_REPORT_LEN]) -> Self {
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

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ThermalTelemetry {
    pub internal_decic: i16,
    pub external1_decic: i16,
}

impl ThermalTelemetry {
    pub fn to_bytes(self) -> [u8; THERMAL_REPORT_LEN] {
        let mut buf = [0u8; THERMAL_REPORT_LEN];
        Report::new(&mut buf)
            .field(self.internal_decic)
            .field(self.external1_decic);
        buf
    }

    pub fn from_bytes(bytes: [u8; THERMAL_REPORT_LEN]) -> Self {
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
