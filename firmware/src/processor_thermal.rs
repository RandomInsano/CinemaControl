//! RP2040 on-die temperature sensor (ADC channel 4), refreshed on its own
//! cadence independently of the PSU's SMBus sensors in `smbus.rs`.

use defmt::warn;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::Watch;
use embassy_time::{Duration, Timer};
use mcu_hal::adc;

use protocol::ProcessorThermalTelemetry;

use crate::board::{ProcessorThermalAdc, ProcessorThermalChannel};

const REFRESH_INTERVAL: Duration = Duration::from_millis(250);

pub static PROCESSOR_THERMAL_TELEMETRY: Watch<
    CriticalSectionRawMutex,
    ProcessorThermalTelemetry,
    1,
> = Watch::new_with(ProcessorThermalTelemetry { decic: 0 });

#[embassy_executor::task]
pub async fn task(mut adc: ProcessorThermalAdc, mut channel: ProcessorThermalChannel) -> ! {
    let mut last_sent = ProcessorThermalTelemetry::default();

    loop {
        match read_processor_thermal_decic(&mut adc, &mut channel).await {
            Ok(decic) => {
                let telemetry = ProcessorThermalTelemetry { decic };
                if telemetry != last_sent {
                    PROCESSOR_THERMAL_TELEMETRY.sender().send(telemetry);
                    last_sent = telemetry;
                }
            }
            Err(e) => warn!("RP2040 chip temperature read failed: {}", e),
        }
        Timer::after(REFRESH_INTERVAL).await;
    }
}

async fn read_processor_thermal_decic(
    adc: &mut ProcessorThermalAdc,
    channel: &mut ProcessorThermalChannel,
) -> Result<i16, adc::Error> {
    let raw = adc.read(channel).await?;

    // RP2040 datasheet §4.9.4: Vbe-based sensor, Vtemp = 0.706V at 27°C,
    // slope -1.721mV/°C.
    let voltage = f32::from(raw) * 3.3 / 4096.0;
    let celsius = 27.0 - (voltage - 0.706) / 0.001721;
    Ok((celsius * 10.0) as i16)
}
