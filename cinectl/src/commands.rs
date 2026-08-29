use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result};
use hidapi::{HidApi, HidDevice};

use crate::device::Board;
use crate::report::{
    self, BRIGHTNESS_FEATURE_REPORT_LEN, BRIGHTNESS_INPUT_REPORT_LEN, POWER_FEATURE_REPORT_LEN,
    POWER_INPUT_REPORT_LEN, PowerTelemetry, THERMAL_FEATURE_REPORT_LEN, THERMAL_INPUT_REPORT_LEN,
    ThermalTelemetry,
};

pub fn list(boards: &[Board]) -> Result<()> {
    if boards.is_empty() {
        println!("no CinemaControl devices found");
        return Ok(());
    }
    for (index, board) in boards.iter().enumerate() {
        println!("[{index}] serial {:?}", board.serial);
    }
    Ok(())
}

pub fn get_brightness(api: &HidApi, board: &Board) -> Result<()> {
    let device = open(api, &board.brightness_path)?;
    let mut buf = [0u8; BRIGHTNESS_FEATURE_REPORT_LEN];
    device
        .get_feature_report(&mut buf)
        .context("reading brightness feature report")?;
    println!("{}", report::brightness_from_bytes([buf[1], buf[2]]));
    Ok(())
}

pub fn set_brightness(api: &HidApi, board: &Board, value: u16) -> Result<()> {
    let device = open(api, &board.brightness_path)?;
    let report = report::brightness_feature_report(value);
    device
        .send_feature_report(&report)
        .context("writing brightness feature report")?;
    println!(
        "brightness set to {}",
        report::brightness_from_bytes([report[1], report[2]])
    );
    Ok(())
}

pub fn get_psu(api: &HidApi, board: &Board) -> Result<()> {
    let power_device = open(api, &board.power_path)?;
    let mut power_buf = [0u8; POWER_FEATURE_REPORT_LEN];
    power_device
        .get_feature_report(&mut power_buf)
        .context("reading power feature report")?;
    let power = PowerTelemetry::from_bytes(power_buf[1..].try_into().unwrap());

    let thermal_device = open(api, &board.thermal_path)?;
    let mut thermal_buf = [0u8; THERMAL_FEATURE_REPORT_LEN];
    thermal_device
        .get_feature_report(&mut thermal_buf)
        .context("reading thermal feature report")?;
    let thermal = ThermalTelemetry::from_bytes(thermal_buf[1..].try_into().unwrap());

    println!("{power}  {thermal}");
    Ok(())
}

/// Streams brightness, power, and thermal Input reports as they arrive —
/// one blocking reader thread per HID interface, since the firmware only
/// pushes a report when a value actually changes (see
/// `firmware/src/hid.rs`/`firmware/src/smbus.rs`), so a plain blocking
/// `read` per thread is already exactly "watch for changes", no polling
/// loop needed. Power and thermal are separate interfaces updating at very
/// different rates (see `firmware/src/hid.rs`'s module doc comment), so
/// each is reported on its own line the moment it changes, rather than
/// merged into one combined line that would also reprint whichever field
/// *didn't* just change. Runs until interrupted (Ctrl-C).
pub fn watch(api: &HidApi, board: &Board) -> Result<()> {
    let brightness_device = open(api, &board.brightness_path)?;
    let power_device = open(api, &board.power_path)?;
    let thermal_device = open(api, &board.thermal_path)?;

    let (tx, rx) = mpsc::channel::<String>();

    spawn_reader(
        brightness_device,
        tx.clone(),
        BRIGHTNESS_INPUT_REPORT_LEN,
        |bytes| {
            format!(
                "brightness: {}",
                report::brightness_from_bytes([bytes[0], bytes[1]])
            )
        },
    );
    spawn_reader(power_device, tx.clone(), POWER_INPUT_REPORT_LEN, |bytes| {
        let power = PowerTelemetry::from_bytes(bytes.try_into().unwrap());
        format!("power: {power}")
    });
    spawn_reader(thermal_device, tx, THERMAL_INPUT_REPORT_LEN, |bytes| {
        let thermal = ThermalTelemetry::from_bytes(bytes.try_into().unwrap());
        format!("thermal: {thermal}")
    });

    for line in rx {
        println!("{line}");
    }
    Ok(())
}

/// Runs `format` against every Input report `device` produces, on its own
/// thread, sending the formatted lines to `tx` until the device errors (e.g.
/// unplugged), at which point the thread reports that and exits.
fn spawn_reader(
    device: HidDevice,
    tx: mpsc::Sender<String>,
    report_len: usize,
    format: impl Fn(&[u8]) -> String + Send + 'static,
) {
    thread::spawn(move || {
        let mut buf = vec![0u8; report_len];
        loop {
            match device.read(&mut buf) {
                Ok(_) => {
                    if tx.send(format(&buf)).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(format!("stopped watching: {e}"));
                    return;
                }
            }
        }
    });
}

fn open(api: &HidApi, path: &std::ffi::CStr) -> Result<HidDevice> {
    api.open_path(path)
        .with_context(|| format!("opening HID interface {path:?}"))
}
