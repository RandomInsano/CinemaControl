//! Finding connected CinemaControl boards and telling them apart.
//!
//! The firmware exposes three HID interfaces per board (brightness, power,
//! thermal — see `firmware/src/hid.rs`) under one composite USB device.
//! Grouping/ordering here is by USB serial number, which the firmware
//! derives from the RP2040's attached flash chip's factory-programmed
//! unique ID — so every board is distinct out of the box, no provisioning
//! step needed.
//!
//! Interfaces are told apart by USB interface number, not usage page — the
//! power and thermal interfaces intentionally share one usage page (see
//! `firmware/src/hid.rs`'s `THERMAL_REPORT_DESCRIPTOR` doc comment), and
//! `hidapi`'s own docs note `usage_page()` isn't even available on Linux's
//! libusb backend. Interface number is fixed by the order `hid::init`
//! builds the interfaces in and is reliable everywhere.

use std::collections::BTreeMap;
use std::ffi::CString;

use anyhow::{Context, Result, bail};
use hidapi::HidApi;

pub const VENDOR_ID: u16 = 0x1209;
pub const PRODUCT_ID: u16 = 0xCC02;

// Interface numbers, fixed by the order `hid::init` (firmware/src/hid.rs)
// registers its interfaces: brightness, then power, then thermal.
const BRIGHTNESS_INTERFACE: i32 = 0;
const POWER_INTERFACE: i32 = 1;
const THERMAL_INTERFACE: i32 = 2;

/// One physical CinemaControl board.
pub struct Board {
    pub serial: String,
    pub brightness_path: CString,
    pub power_path: CString,
    pub thermal_path: CString,
}

/// One board's interface paths as they're discovered — `None` until that
/// interface's entry in [`discover`]'s scan is seen.
type PartialBoard = (Option<CString>, Option<CString>, Option<CString>);

/// Every connected CinemaControl board, sorted by ascending [`Board::serial`]
/// — used as this tool's device index.
pub fn discover(api: &HidApi) -> Result<Vec<Board>> {
    let mut by_serial: BTreeMap<String, PartialBoard> = BTreeMap::new();

    for info in api.device_list() {
        if info.vendor_id() != VENDOR_ID || info.product_id() != PRODUCT_ID {
            continue;
        }

        let serial = info.serial_number().unwrap_or_default().to_string();
        let slot = by_serial.entry(serial).or_default();
        match info.interface_number() {
            BRIGHTNESS_INTERFACE => slot.0 = Some(info.path().to_owned()),
            POWER_INTERFACE => slot.1 = Some(info.path().to_owned()),
            THERMAL_INTERFACE => slot.2 = Some(info.path().to_owned()),
            other => bail!("unexpected interface number {other} on a CinemaControl device"),
        }
    }

    by_serial
        .into_iter()
        .map(|(serial, (brightness, power, thermal))| {
            Ok(Board {
                brightness_path: brightness
                    .with_context(|| format!("board {serial:?} has no brightness interface"))?,
                power_path: power
                    .with_context(|| format!("board {serial:?} has no power interface"))?,
                thermal_path: thermal
                    .with_context(|| format!("board {serial:?} has no thermal interface"))?,
                serial,
            })
        })
        .collect()
}
