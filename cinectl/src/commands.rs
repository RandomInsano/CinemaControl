use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result};
use hidapi::{HidApi, HidDevice};

use protocol::{
    BRIGHTNESS_REPORT_LEN, POWER_REPORT_LEN, PowerTelemetry, THERMAL_REPORT_LEN, ThermalTelemetry,
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
    let device = open(api, &board.brightness_path)?;
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
    let power = read_power(api, board)?;
    let thermal = read_thermal(api, board)?;
    println!("{power}  {thermal}");
    Ok(())
}

fn read_brightness(api: &HidApi, board: &Board) -> Result<u16> {
    read_feature(
        api,
        &board.brightness_path,
        BRIGHTNESS_REPORT_LEN,
        "brightness",
        |payload| report::brightness_from_bytes(payload.try_into().unwrap()),
    )
}

fn read_power(api: &HidApi, board: &Board) -> Result<PowerTelemetry> {
    read_feature(
        api,
        &board.power_path,
        POWER_REPORT_LEN,
        "power",
        |payload| PowerTelemetry::from_bytes(payload.try_into().unwrap()),
    )
}

fn read_thermal(api: &HidApi, board: &Board) -> Result<ThermalTelemetry> {
    read_feature(
        api,
        &board.thermal_path,
        THERMAL_REPORT_LEN,
        "thermal",
        |payload| ThermalTelemetry::from_bytes(payload.try_into().unwrap()),
    )
}

fn read_feature<T>(
    api: &HidApi,
    path: &std::ffi::CStr,
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
    Thermal(ThermalTelemetry),
    Error(String),
}

pub fn watch(api: &HidApi, board: &Board, combined: bool) -> Result<()> {
    let brightness_device = open(api, &board.brightness_path)?;
    let power_device = open(api, &board.power_path)?;
    let thermal_device = open(api, &board.thermal_path)?;

    let (tx, rx) = mpsc::channel::<Update>();

    spawn_reader(
        brightness_device,
        tx.clone(),
        BRIGHTNESS_REPORT_LEN,
        |bytes| Update::Brightness(report::brightness_from_bytes([bytes[0], bytes[1]])),
    );
    spawn_reader(power_device, tx.clone(), POWER_REPORT_LEN, |bytes| {
        Update::Power(PowerTelemetry::from_bytes(bytes.try_into().unwrap()))
    });
    spawn_reader(thermal_device, tx, THERMAL_REPORT_LEN, |bytes| {
        Update::Thermal(ThermalTelemetry::from_bytes(bytes.try_into().unwrap()))
    });

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
            Update::Thermal(t) => println!("thermal: {t}"),
            Update::Error(e) => println!("{e}"),
        }
    }
    Ok(())
}

fn watch_combined(api: &HidApi, board: &Board, rx: mpsc::Receiver<Update>) -> Result<()> {
    let mut brightness = read_brightness(api, board)?;
    let mut power = read_power(api, board)?;
    let mut thermal = read_thermal(api, board)?;
    println!("brightness: {brightness:4}  {power}  {thermal}");

    for update in rx {
        match update {
            Update::Brightness(v) => brightness = v,
            Update::Power(p) => power = p,
            Update::Thermal(t) => thermal = t,
            Update::Error(e) => {
                println!("{e}");
                continue;
            }
        }
        println!("brightness: {brightness:4}  {power}  {thermal}");
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
