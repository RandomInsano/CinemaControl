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
        report::brightness_from_bytes([report[1], report[2]])
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
    let device = open(api, &board.brightness_path)?;
    let mut buf = [0u8; BRIGHTNESS_FEATURE_REPORT_LEN];
    device
        .get_feature_report(&mut buf)
        .context("reading brightness feature report")?;
    Ok(report::brightness_from_bytes([buf[1], buf[2]]))
}

fn read_power(api: &HidApi, board: &Board) -> Result<PowerTelemetry> {
    let device = open(api, &board.power_path)?;
    let mut buf = [0u8; POWER_FEATURE_REPORT_LEN];
    device
        .get_feature_report(&mut buf)
        .context("reading power feature report")?;
    Ok(PowerTelemetry::from_bytes(buf[1..].try_into().unwrap()))
}

fn read_thermal(api: &HidApi, board: &Board) -> Result<ThermalTelemetry> {
    let device = open(api, &board.thermal_path)?;
    let mut buf = [0u8; THERMAL_FEATURE_REPORT_LEN];
    device
        .get_feature_report(&mut buf)
        .context("reading thermal feature report")?;
    Ok(ThermalTelemetry::from_bytes(buf[1..].try_into().unwrap()))
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
        BRIGHTNESS_INPUT_REPORT_LEN,
        |bytes| Update::Brightness(report::brightness_from_bytes([bytes[0], bytes[1]])),
    );
    spawn_reader(power_device, tx.clone(), POWER_INPUT_REPORT_LEN, |bytes| {
        Update::Power(PowerTelemetry::from_bytes(bytes.try_into().unwrap()))
    });
    spawn_reader(thermal_device, tx, THERMAL_INPUT_REPORT_LEN, |bytes| {
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
