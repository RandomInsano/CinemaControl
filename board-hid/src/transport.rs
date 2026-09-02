//! Opening HID interfaces and reading feature reports off them.

use std::ffi::{CStr, CString};

use anyhow::{Context, Result};
use hidapi::{HidApi, HidDevice};

/// A board only needs to expose *some* interface to be discovered (see
/// `device::discover`) — this is where a board missing one specific
/// interface (e.g. older firmware without `processor_thermal`) surfaces as a
/// clear error instead of a HID open failure.
pub fn require_path<'a>(path: &'a Option<CString>, label: &str) -> Result<&'a CStr> {
    path.as_deref()
        .with_context(|| format!("device has no {label} interface"))
}

pub fn open(api: &HidApi, path: &CStr) -> Result<HidDevice> {
    api.open_path(path)
        .with_context(|| format!("opening HID interface {path:?}"))
}

pub fn read_feature<T>(
    api: &HidApi,
    path: &CStr,
    report_len: usize,
    label: &str,
    decode: impl FnOnce(&[u8]) -> T,
) -> Result<T> {
    let device = open(api, path)?;
    let mut buf = vec![0u8; report_len + 1];
    device
        .get_feature_report(&mut buf)
        .with_context(|| format!("reading {label} feature report"))?;
    Ok(decode(&buf[1..]))
}
