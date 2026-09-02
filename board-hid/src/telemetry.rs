//! One-shot feature-report reads for each of a board's interfaces, plus a
//! streaming counterpart that reads pushed input reports instead — see
//! `transport::stream_input`.

use std::sync::mpsc::Receiver;

use anyhow::Result;
use hidapi::HidApi;
use protocol::{
    BRIGHTNESS_REPORT_LEN, POWER_REPORT_LEN, POWER_THERMAL_REPORT_LEN,
    PROCESSOR_THERMAL_REPORT_LEN, PowerTelemetry, PowerThermalTelemetry, ProcessorThermalTelemetry,
};

use crate::device::Board;
use crate::report;
use crate::transport::{open, read_feature, require_path, stream_input};

fn decode_brightness(payload: &[u8]) -> u16 {
    report::brightness_from_bytes(
        payload
            .try_into()
            .expect("read_feature/stream_input always hand decode() exactly report_len bytes"),
    )
}

fn decode_power(payload: &[u8]) -> PowerTelemetry {
    PowerTelemetry::from_bytes(
        payload
            .try_into()
            .expect("read_feature/stream_input always hand decode() exactly report_len bytes"),
    )
}

fn decode_power_thermal(payload: &[u8]) -> PowerThermalTelemetry {
    PowerThermalTelemetry::from_bytes(
        payload
            .try_into()
            .expect("read_feature/stream_input always hand decode() exactly report_len bytes"),
    )
}

fn decode_processor_thermal(payload: &[u8]) -> ProcessorThermalTelemetry {
    ProcessorThermalTelemetry::from_bytes(
        payload
            .try_into()
            .expect("read_feature/stream_input always hand decode() exactly report_len bytes"),
    )
}

pub fn read_brightness(api: &HidApi, board: &Board) -> Result<u16> {
    read_feature(
        api,
        require_path(&board.brightness_path, "brightness")?,
        BRIGHTNESS_REPORT_LEN,
        "brightness",
        decode_brightness,
    )
}

pub fn read_power(api: &HidApi, board: &Board) -> Result<PowerTelemetry> {
    read_feature(
        api,
        require_path(&board.power_path, "power")?,
        POWER_REPORT_LEN,
        "power",
        decode_power,
    )
}

pub fn read_power_thermal(api: &HidApi, board: &Board) -> Result<PowerThermalTelemetry> {
    read_feature(
        api,
        require_path(&board.power_thermal_path, "thermal")?,
        POWER_THERMAL_REPORT_LEN,
        "thermal",
        decode_power_thermal,
    )
}

pub fn read_processor_thermal(api: &HidApi, board: &Board) -> Result<ProcessorThermalTelemetry> {
    read_feature(
        api,
        require_path(&board.processor_thermal_path, "chip temperature")?,
        PROCESSOR_THERMAL_REPORT_LEN,
        "chip temperature",
        decode_processor_thermal,
    )
}

/// Streams brightness input reports pushed by the device — see
/// `transport::stream_input`. Errs immediately if the board has no
/// brightness interface or it can't be opened; the returned channel then
/// yields a decoded value each time the device's brightness actually
/// changes (including in response to a feature-report write from us).
pub fn stream_brightness(api: &HidApi, board: &Board) -> Result<Receiver<Result<u16>>> {
    let device = open(api, require_path(&board.brightness_path, "brightness")?)?;
    Ok(stream_input(
        device,
        BRIGHTNESS_REPORT_LEN,
        "brightness",
        decode_brightness,
    ))
}

pub fn stream_power(api: &HidApi, board: &Board) -> Result<Receiver<Result<PowerTelemetry>>> {
    let device = open(api, require_path(&board.power_path, "power")?)?;
    Ok(stream_input(
        device,
        POWER_REPORT_LEN,
        "power",
        decode_power,
    ))
}

pub fn stream_power_thermal(
    api: &HidApi,
    board: &Board,
) -> Result<Receiver<Result<PowerThermalTelemetry>>> {
    let device = open(api, require_path(&board.power_thermal_path, "thermal")?)?;
    Ok(stream_input(
        device,
        POWER_THERMAL_REPORT_LEN,
        "thermal",
        decode_power_thermal,
    ))
}

pub fn stream_processor_thermal(
    api: &HidApi,
    board: &Board,
) -> Result<Receiver<Result<ProcessorThermalTelemetry>>> {
    let device = open(
        api,
        require_path(&board.processor_thermal_path, "chip temperature")?,
    )?;
    Ok(stream_input(
        device,
        PROCESSOR_THERMAL_REPORT_LEN,
        "chip temperature",
        decode_processor_thermal,
    ))
}
