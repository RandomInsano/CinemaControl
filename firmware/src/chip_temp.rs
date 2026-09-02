//! RP2040 on-die temperature sensor (ADC channel 4), refreshed on its own
//! cadence independently of the PSU's SMBus sensors in `smbus.rs`.

use defmt::warn;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::Watch;
use embassy_time::{Duration, Timer};
use mcu_hal::adc;

use protocol::ChipTemperature;

use crate::board::{ChipTempAdc, ChipTempChannel};

const REFRESH_INTERVAL: Duration = Duration::from_millis(250);

pub static CHIP_TEMPERATURE: Watch<CriticalSectionRawMutex, ChipTemperature, 1> =
    Watch::new_with(ChipTemperature { decic: 0 });

#[embassy_executor::task]
pub async fn task(mut adc: ChipTempAdc, mut channel: ChipTempChannel) -> ! {
    let mut last_sent = ChipTemperature::default();

    loop {
        match read_chip_temp_decic(&mut adc, &mut channel).await {
            Ok(decic) => {
                let telemetry = ChipTemperature { decic };
                if telemetry != last_sent {
                    CHIP_TEMPERATURE.sender().send(telemetry);
                    last_sent = telemetry;
                }
            }
            Err(e) => warn!("RP2040 chip temperature read failed: {}", e),
        }
        Timer::after(REFRESH_INTERVAL).await;
    }
}

async fn read_chip_temp_decic(
    adc: &mut ChipTempAdc,
    channel: &mut ChipTempChannel,
) -> Result<i16, adc::Error> {
    let raw = adc.read(channel).await?;

    // RP2040 datasheet §4.9.4: Vbe-based sensor, Vtemp = 0.706V at 27°C,
    // slope -1.721mV/°C.
    let voltage = f32::from(raw) * 3.3 / 4096.0;
    let celsius = 27.0 - (voltage - 0.706) / 0.001721;
    Ok((celsius * 10.0) as i16)
}
