use std::ffi::{CStr, CString};
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result};
use hidapi::{HidApi, HidDevice};

use protocol::{
    BRIGHTNESS_REPORT_LEN, POWER_REPORT_LEN, POWER_THERMAL_REPORT_LEN,
    PROCESSOR_THERMAL_REPORT_LEN, PowerTelemetry, PowerThermalTelemetry, ProcessorThermalTelemetry,
};

use crate::device::Board;
use crate::report;

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
    println!("{}", read_brightness(api, board)?);
    Ok(())
}

pub fn set_brightness(api: &HidApi, board: &Board, value: u16) -> Result<()> {
    let device = open(api, require_path(&board.brightness_path, "brightness")?)?;
    let report = report::brightness_feature_report(value);
    device
        .send_feature_report(&report)
        .context("writing brightness feature report")?;
    println!(
        "brightness set to {}",
        report::brightness_from_bytes(feature_payload(&report).try_into().unwrap())
    );
    Ok(())
}

pub fn get_psu(api: &HidApi, board: &Board) -> Result<()> {
    let power = read_power(api, board).unwrap_or_default();
    let power_thermal = read_power_thermal(api, board).unwrap_or_default();
    let processor_thermal = read_processor_thermal(api, board).unwrap_or_default();
    println!("{power}  {power_thermal}  {processor_thermal}");
    Ok(())
}

fn read_brightness(api: &HidApi, board: &Board) -> Result<u16> {
    read_feature(
        api,
        require_path(&board.brightness_path, "brightness")?,
        BRIGHTNESS_REPORT_LEN,
        "brightness",
        |payload| report::brightness_from_bytes(payload.try_into().unwrap()),
    )
}

fn read_power(api: &HidApi, board: &Board) -> Result<PowerTelemetry> {
    read_feature(
        api,
        require_path(&board.power_path, "power")?,
        POWER_REPORT_LEN,
        "power",
        |payload| PowerTelemetry::from_bytes(payload.try_into().unwrap()),
    )
}

fn read_power_thermal(api: &HidApi, board: &Board) -> Result<PowerThermalTelemetry> {
    read_feature(
        api,
        require_path(&board.power_thermal_path, "thermal")?,
        POWER_THERMAL_REPORT_LEN,
        "thermal",
        |payload| PowerThermalTelemetry::from_bytes(payload.try_into().unwrap()),
    )
}

fn read_processor_thermal(api: &HidApi, board: &Board) -> Result<ProcessorThermalTelemetry> {
    read_feature(
        api,
        require_path(&board.processor_thermal_path, "chip temperature")?,
        PROCESSOR_THERMAL_REPORT_LEN,
        "chip temperature",
        |payload| ProcessorThermalTelemetry::from_bytes(payload.try_into().unwrap()),
    )
}

/// A board only needs to expose *some* interface to be discovered (see
/// `device::discover`) — this is where a board missing one specific
/// interface (e.g. older firmware without `processor_thermal`) surfaces as a
/// clear error instead of a HID open failure.
fn require_path<'a>(path: &'a Option<CString>, label: &str) -> Result<&'a CStr> {
    path.as_deref()
        .with_context(|| format!("device has no {label} interface"))
}

fn read_feature<T>(
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
    Ok(decode(feature_payload(&buf)))
}

fn feature_payload(buf: &[u8]) -> &[u8] {
    &buf[1..]
}

enum Update {
    Brightness(u16),
    Power(PowerTelemetry),
    PowerThermal(PowerThermalTelemetry),
    ProcessorThermal(ProcessorThermalTelemetry),
    Error(String),
}

pub fn watch(api: &HidApi, board: &Board, combined: bool) -> Result<()> {
    let brightness_device = open(api, require_path(&board.brightness_path, "brightness")?)?;

    let (tx, rx) = mpsc::channel::<Update>();

    spawn_reader(
        brightness_device,
        tx.clone(),
        BRIGHTNESS_REPORT_LEN,
        |bytes| Update::Brightness(report::brightness_from_bytes([bytes[0], bytes[1]])),
    );
    if let Some(path) = board.power_path.as_deref() {
        let power_device = open(api, path)?;
        spawn_reader(power_device, tx.clone(), POWER_REPORT_LEN, |bytes| {
            Update::Power(PowerTelemetry::from_bytes(bytes.try_into().unwrap()))
        });
    }
    if let Some(path) = board.power_thermal_path.as_deref() {
        let power_thermal_device = open(api, path)?;
        spawn_reader(
            power_thermal_device,
            tx.clone(),
            POWER_THERMAL_REPORT_LEN,
            |bytes| {
                Update::PowerThermal(PowerThermalTelemetry::from_bytes(bytes.try_into().unwrap()))
            },
        );
    }
    if let Some(path) = board.processor_thermal_path.as_deref() {
        let processor_thermal_device = open(api, path)?;
        spawn_reader(
            processor_thermal_device,
            tx.clone(),
            PROCESSOR_THERMAL_REPORT_LEN,
            |bytes| {
                Update::ProcessorThermal(ProcessorThermalTelemetry::from_bytes(
                    bytes.try_into().unwrap(),
                ))
            },
        );
    }
    drop(tx);

    if combined {
        watch_combined(api, board, rx)
    } else {
        watch_separate(rx)
    }
}

fn watch_separate(rx: mpsc::Receiver<Update>) -> Result<()> {
    for update in rx {
        match update {
            Update::Brightness(v) => println!("brightness: {v:4}"),
            Update::Power(p) => println!("power: {p}"),
            Update::PowerThermal(t) => println!("thermal: {t}"),
            Update::ProcessorThermal(c) => println!("chip temp: {c}"),
            Update::Error(e) => println!("{e}"),
        }
    }
    Ok(())
}

fn watch_combined(api: &HidApi, board: &Board, rx: mpsc::Receiver<Update>) -> Result<()> {
    let mut brightness = read_brightness(api, board)?;
    let mut power = read_power(api, board).unwrap_or_default();
    let mut power_thermal = read_power_thermal(api, board).unwrap_or_default();
    let mut processor_thermal = read_processor_thermal(api, board).unwrap_or_default();
    println!("brightness: {brightness:4}  {power}  {power_thermal}  {processor_thermal}");

    for update in rx {
        match update {
            Update::Brightness(v) => brightness = v,
            Update::Power(p) => power = p,
            Update::PowerThermal(t) => power_thermal = t,
            Update::ProcessorThermal(c) => processor_thermal = c,
            Update::Error(e) => {
                println!("{e}");
                continue;
            }
        }
        println!("brightness: {brightness:4}  {power}  {power_thermal}  {processor_thermal}");
    }
    Ok(())
}

fn spawn_reader(
    device: HidDevice,
    tx: mpsc::Sender<Update>,
    report_len: usize,
    format: impl Fn(&[u8]) -> Update + Send + 'static,
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
                    let _ = tx.send(Update::Error(format!("stopped watching: {e}")));
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
