//! One-shot feature-report reads for each of a board's interfaces.

use anyhow::Result;
use hidapi::HidApi;
use protocol::{
    BRIGHTNESS_REPORT_LEN, POWER_REPORT_LEN, POWER_THERMAL_REPORT_LEN,
    PROCESSOR_THERMAL_REPORT_LEN, PowerTelemetry, PowerThermalTelemetry, ProcessorThermalTelemetry,
};

use crate::device::Board;
use crate::report;
use crate::transport::{read_feature, require_path};

pub fn read_brightness(api: &HidApi, board: &Board) -> Result<u16> {
    read_feature(
        api,
        require_path(&board.brightness_path, "brightness")?,
        BRIGHTNESS_REPORT_LEN,
        "brightness",
        |payload| {
            report::brightness_from_bytes(
                payload
                    .try_into()
                    .expect("read_feature always hands decode() exactly report_len bytes"),
            )
        },
    )
}

pub fn read_power(api: &HidApi, board: &Board) -> Result<PowerTelemetry> {
    read_feature(
        api,
        require_path(&board.power_path, "power")?,
        POWER_REPORT_LEN,
        "power",
        |payload| {
            PowerTelemetry::from_bytes(
                payload
                    .try_into()
                    .expect("read_feature always hands decode() exactly report_len bytes"),
            )
        },
    )
}

pub fn read_power_thermal(api: &HidApi, board: &Board) -> Result<PowerThermalTelemetry> {
    read_feature(
        api,
        require_path(&board.power_thermal_path, "thermal")?,
        POWER_THERMAL_REPORT_LEN,
        "thermal",
        |payload| {
            PowerThermalTelemetry::from_bytes(
                payload
                    .try_into()
                    .expect("read_feature always hands decode() exactly report_len bytes"),
            )
        },
    )
}

pub fn read_processor_thermal(api: &HidApi, board: &Board) -> Result<ProcessorThermalTelemetry> {
    read_feature(
        api,
        require_path(&board.processor_thermal_path, "chip temperature")?,
        PROCESSOR_THERMAL_REPORT_LEN,
        "chip temperature",
        |payload| {
            ProcessorThermalTelemetry::from_bytes(
                payload
                    .try_into()
                    .expect("read_feature always hands decode() exactly report_len bytes"),
            )
        },
    )
}
