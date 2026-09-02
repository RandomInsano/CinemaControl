//! Opening HID interfaces and reading feature reports (or streaming input
//! reports) off them.

use std::ffi::{CStr, CString};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use anyhow::{Context, Result, anyhow};
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

/// Spawns a background thread that blocking-reads `device`'s input reports
/// one at a time, decodes each, and sends it down the returned channel —
/// the host-side other half of the firmware's `Watch::changed()`-gated
/// push (see `firmware/src/hid.rs`'s `*_report_task`s): a report only
/// arrives when the value actually changes, not on a fixed poll.
///
/// The thread — and the channel — ends the first time a read comes back
/// short, malformed, or erroring (typically because the device was
/// unplugged), or once the receiver is dropped. There's no reconnect
/// logic here; callers track board presence separately (via `discover`)
/// and just drop the receiver when a board is gone.
pub fn stream_input<T: Send + 'static>(
    device: HidDevice,
    report_len: usize,
    label: &'static str,
    decode: impl Fn(&[u8]) -> T + Send + 'static,
) -> Receiver<Result<T>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = vec![0u8; report_len];
        loop {
            let outcome = match device.read(&mut buf) {
                Ok(n) if n == report_len => Ok(decode(&buf[..n])),
                Ok(n) => Err(anyhow!(
                    "short {label} input report ({n} of {report_len} bytes)"
                )),
                Err(e) => {
                    Err(anyhow::Error::new(e).context(format!("reading {label} input report")))
                }
            };
            let stop = outcome.is_err();
            if tx.send(outcome).is_err() || stop {
                return;
            }
        }
    });
    rx
}
