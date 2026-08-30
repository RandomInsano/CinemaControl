//! Finding connected CinemaControl boards and telling them apart.

use std::collections::BTreeMap;
use std::ffi::CString;

use anyhow::{Context, Result, bail};
use hidapi::HidApi;

pub const VENDOR_ID: u16 = 0x1209;
pub const PRODUCT_ID: u16 = 0xCC02;

const BRIGHTNESS_INTERFACE: i32 = 0;
const POWER_INTERFACE: i32 = 1;
const THERMAL_INTERFACE: i32 = 2;

pub struct Board {
    pub serial: String,
    pub brightness_path: CString,
    pub power_path: CString,
    pub thermal_path: CString,
}

type PartialBoard = (Option<CString>, Option<CString>, Option<CString>);

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
