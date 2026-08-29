//! Finding connected CinemaControl boards and telling them apart.
//!
//! The firmware exposes two HID interfaces per board (brightness + PSU
//! telemetry, see `firmware/src/hid.rs`) under one composite USB device.
//! Grouping/ordering here is by USB serial number, which the firmware
//! derives from the RP2040's attached flash chip's factory-programmed
//! unique ID — so every board is distinct out of the box, no provisioning
//! step needed.

use std::collections::BTreeMap;
use std::ffi::CString;

use anyhow::{Context, Result, bail};
use hidapi::HidApi;

pub const VENDOR_ID: u16 = 0x1209;
pub const PRODUCT_ID: u16 = 0xCC02;

// Usage pages from the report descriptors in firmware/src/hid.rs.
const BRIGHTNESS_USAGE_PAGE: u16 = 0x80; // Monitor
const PSU_USAGE_PAGE: u16 = 0x84; // Power Device

/// One physical CinemaControl board.
pub struct Board {
    pub serial: String,
    pub brightness_path: CString,
    pub psu_path: CString,
}

/// Every connected CinemaControl board, sorted by ascending [`Board::serial`]
/// — used as this tool's device index.
pub fn discover(api: &HidApi) -> Result<Vec<Board>> {
    let mut by_serial: BTreeMap<String, (Option<CString>, Option<CString>)> = BTreeMap::new();

    for info in api.device_list() {
        if info.vendor_id() != VENDOR_ID || info.product_id() != PRODUCT_ID {
            continue;
        }

        let serial = info.serial_number().unwrap_or_default().to_string();
        let slot = by_serial.entry(serial).or_default();
        match info.usage_page() {
            BRIGHTNESS_USAGE_PAGE => slot.0 = Some(info.path().to_owned()),
            PSU_USAGE_PAGE => slot.1 = Some(info.path().to_owned()),
            other => bail!("unexpected usage page 0x{other:02x} on a CinemaControl interface"),
        }
    }

    by_serial
        .into_iter()
        .map(|(serial, (brightness, psu))| {
            Ok(Board {
                brightness_path: brightness
                    .with_context(|| format!("board {serial:?} has no brightness interface"))?,
                psu_path: psu.with_context(|| format!("board {serial:?} has no PSU interface"))?,
                serial,
            })
        })
        .collect()
}
