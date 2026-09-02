//! Finding connected CinemaControl boards and telling them apart.

use std::collections::BTreeMap;
use std::ffi::CString;

use anyhow::{Result, bail};
use hidapi::HidApi;

const BRIGHTNESS_INTERFACE: i32 = 0;
const POWER_INTERFACE: i32 = 1;
const POWER_THERMAL_INTERFACE: i32 = 2;
const PROCESSOR_THERMAL_INTERFACE: i32 = 3;

/// Each path is `None` when the connected board's firmware predates that
/// interface (e.g. `processor_thermal_path` on a board flashed before it
/// existed) — a board only needs to expose *some* interface to be
/// discovered at all.
pub struct Board {
    pub serial: String,
    pub brightness_path: Option<CString>,
    pub power_path: Option<CString>,
    pub power_thermal_path: Option<CString>,
    pub processor_thermal_path: Option<CString>,
}

type PartialBoard = (
    Option<CString>,
    Option<CString>,
    Option<CString>,
    Option<CString>,
);

pub fn discover(api: &HidApi) -> Result<Vec<Board>> {
    let mut by_serial: BTreeMap<String, PartialBoard> = BTreeMap::new();

    for info in api.device_list() {
        if info.vendor_id() != protocol::VENDOR_ID || info.product_id() != protocol::PRODUCT_ID {
            continue;
        }

        let serial = info.serial_number().unwrap_or_default().to_string();
        let slot = by_serial.entry(serial).or_default();
        match info.interface_number() {
            BRIGHTNESS_INTERFACE => slot.0 = Some(info.path().to_owned()),
            POWER_INTERFACE => slot.1 = Some(info.path().to_owned()),
            POWER_THERMAL_INTERFACE => slot.2 = Some(info.path().to_owned()),
            PROCESSOR_THERMAL_INTERFACE => slot.3 = Some(info.path().to_owned()),
            other => bail!("unexpected interface number {other} on a CinemaControl device"),
        }
    }

    Ok(by_serial
        .into_iter()
        .map(
            |(
                serial,
                (brightness_path, power_path, power_thermal_path, processor_thermal_path),
            )| Board {
                serial,
                brightness_path,
                power_path,
                power_thermal_path,
                processor_thermal_path,
            },
        )
        .collect())
}
