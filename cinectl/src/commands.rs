use std::sync::mpsc;
use std::thread;

use anyhow::{Context, Result};
use hidapi::{HidApi, HidDevice};

use board_hid::device::Board;
use board_hid::report;
use board_hid::telemetry::{
    read_brightness, read_power, read_power_thermal, read_processor_thermal,
};
use board_hid::transport::{open, require_path};
use protocol::{
    BRIGHTNESS_REPORT_LEN, POWER_REPORT_LEN, POWER_THERMAL_REPORT_LEN,
    PROCESSOR_THERMAL_REPORT_LEN, PowerTelemetry, PowerThermalTelemetry, ProcessorThermalTelemetry,
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
    let device = open(api, require_path(&board.brightness_path, "brightness")?)?;
    let report = report::brightness_feature_report(value);
    device
        .send_feature_report(&report)
        .context("writing brightness feature report")?;
    println!(
        "brightness set to {}",
        report::brightness_from_bytes(report[1..].try_into().unwrap())
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
