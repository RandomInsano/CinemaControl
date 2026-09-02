use std::sync::mpsc::{self, Receiver};
use std::thread;

use anyhow::{Context, Result};
use hidapi::HidApi;

use board_hid::device::Board;
use board_hid::report;
use board_hid::telemetry::{
    read_brightness, read_power, read_power_thermal, read_processor_thermal, stream_brightness,
    stream_power, stream_power_thermal, stream_processor_thermal,
};
use board_hid::transport::{open, require_path};
use protocol::{PowerTelemetry, PowerThermalTelemetry, ProcessorThermalTelemetry};

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
    let (tx, rx) = mpsc::channel::<Update>();

    // Brightness is the one interface every CinemaControl firmware has
    // ever shipped with, so its absence fails the whole command; the
    // PSU/thermal interfaces are best-effort, same as `get_psu`.
    forward(
        stream_brightness(api, board)?,
        tx.clone(),
        Update::Brightness,
    );
    if let Ok(power) = stream_power(api, board) {
        forward(power, tx.clone(), Update::Power);
    }
    if let Ok(power_thermal) = stream_power_thermal(api, board) {
        forward(power_thermal, tx.clone(), Update::PowerThermal);
    }
    if let Ok(processor_thermal) = stream_processor_thermal(api, board) {
        forward(processor_thermal, tx.clone(), Update::ProcessorThermal);
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

/// Relays a `board_hid` telemetry stream onto the merged `Update` channel
/// `watch` blocks on, wrapping each value with `variant`. Ends (dropping
/// its `tx` clone) the moment the stream itself ends — the `Err` that
/// signals that is relayed too, then the loop stops on the now-closed
/// `rx`.
fn forward<T: Send + 'static>(
    rx: Receiver<Result<T>>,
    tx: mpsc::Sender<Update>,
    variant: impl Fn(T) -> Update + Send + 'static,
) {
    thread::spawn(move || {
        for result in rx {
            let message = match result {
                Ok(value) => variant(value),
                Err(e) => Update::Error(format!("stopped watching: {e:#}")),
            };
            if tx.send(message).is_err() {
                return;
            }
        }
    });
}
