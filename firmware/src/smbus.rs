//! Telemetry for the LiteOn PA-2311-02A PSU's secondary-side SMBus: a
//! Microchip EMC1403 at 0x4D (thermal) and a TI INA219 at 0x40
//! (voltage/current/power), each on its own task.

use defmt::warn;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::watch::Watch;
use embassy_time::{Delay, Duration, Timer, with_timeout};
use emc1403::{Channel, ConversionRate, Emc1403};
use ina219::Ina219;
use mcu_hal::i2c;

use protocol::{PowerTelemetry, ThermalTelemetry};

use crate::board::SmbusBus;
use crate::shared_i2c::SharedI2c;

const EMC1403_ADDR: u8 = emc1403::address::EMC1403_2_EMC1404_2;
const EMC1403_RATE: ConversionRate = ConversionRate::PerSec4; // 4 conversions/sec

const INA219_ADDR: u8 = ina219::Address::GndGnd as u8;
const INA219_PRECISION: ina219::AdcSetting = ina219::AdcSetting::Average128;
/// Shunt 2.5mOhm, Cal=0x4000, Power_LSB=20mW. See datasheet
const INA219_CURRENT_LSB_MA: u32 = 1;
const INA219_CALIBRATION_RAW: u16 = 0x4000;

const REFRESH_MARGIN_MS: u64 = 14;
const I2C_CYCLE_TIMEOUT: Duration = Duration::from_millis(100);

pub static POWER_TELEMETRY: Watch<CriticalSectionRawMutex, PowerTelemetry, 1> =
    Watch::new_with(PowerTelemetry {
        voltage_mv: 0,
        current_ma: 0,
        power_mw: 0,
    });

pub static THERMAL_TELEMETRY: Watch<CriticalSectionRawMutex, ThermalTelemetry, 1> =
    Watch::new_with(ThermalTelemetry {
        internal_decic: 0,
        external1_decic: 0,
    });

#[embassy_executor::task]
pub async fn ina219_task(bus: &'static Mutex<CriticalSectionRawMutex, SmbusBus>) -> ! {
    Timer::after(Duration::from_secs(2)).await;

    let mut sensor = Ina219::new(
        SharedI2c::new(bus),
        INA219_ADDR,
        INA219_CURRENT_LSB_MA,
        INA219_CALIBRATION_RAW,
    );

    // BADC+SADC conversion time per datasheet S4.1.
    let refresh_interval = refresh_interval_for(2 * INA219_PRECISION.conversion_time_us());

    let mut last_sent = PowerTelemetry::default();

    loop {
        match with_timeout(I2C_CYCLE_TIMEOUT, try_read_power_rail(&mut sensor)).await {
            Ok(Ok((voltage_mv, current_ma, power_mw))) => {
                let telemetry = PowerTelemetry {
                    voltage_mv,
                    current_ma,
                    power_mw,
                };
                if telemetry != last_sent {
                    POWER_TELEMETRY.sender().send(telemetry);
                    last_sent = telemetry;
                }
            }
            Ok(Err(e)) => warn!("INA219 power rail read failed: {}", defmt::Debug2Format(&e)),
            Err(_) => warn!("INA219 power rail read timed out — PSU likely powered down"),
        }
        Timer::after(refresh_interval).await;
    }
}

#[derive(Debug)]
enum PowerRailError {
    Bus(
        #[allow(
            dead_code,
            reason = "read via defmt::Debug2Format, not matched directly"
        )]
        ina219::Error<i2c::Error>,
    ),
    Overflow,
}

impl From<ina219::Error<i2c::Error>> for PowerRailError {
    fn from(e: ina219::Error<i2c::Error>) -> Self {
        Self::Bus(e)
    }
}

async fn try_read_power_rail(
    sensor: &mut Ina219<SharedI2c>,
) -> Result<(u16, i16, u32), PowerRailError> {
    let config = ina219::Configuration {
        bus_adc: INA219_PRECISION,
        shunt_adc: INA219_PRECISION,
        ..ina219::Configuration::default()
    };
    sensor.set_configuration(config).await?;
    sensor.calibrate().await?;

    let voltage = sensor.bus_voltage().await?;
    if voltage.overflow {
        return Err(PowerRailError::Overflow);
    }

    let current_ma = sensor.current_ma().await? as i16;
    let power_mw = sensor.power_mw().await?;

    Ok((voltage.millivolts, current_ma, power_mw))
}

#[embassy_executor::task]
pub async fn emc1403_task(bus: &'static Mutex<CriticalSectionRawMutex, SmbusBus>) -> ! {
    Timer::after(Duration::from_secs(2)).await;

    let refresh_interval = refresh_interval_for(EMC1403_RATE.period_us());

    let mut last_sent = ThermalTelemetry::default();

    loop {
        match with_timeout(I2C_CYCLE_TIMEOUT, try_read_thermal(bus)).await {
            Ok(Ok((internal_decic, external1_decic))) => {
                let telemetry = ThermalTelemetry {
                    internal_decic,
                    external1_decic,
                };
                if telemetry != last_sent {
                    THERMAL_TELEMETRY.sender().send(telemetry);
                    last_sent = telemetry;
                }
            }
            Ok(Err(e)) => warn!("EMC1403 thermal read failed: {}", defmt::Debug2Format(&e)),
            Err(_) => warn!("EMC1403 thermal read timed out — PSU likely powered down"),
        }
        Timer::after(refresh_interval).await;
    }
}

fn refresh_interval_for(conversion_us: u32) -> Duration {
    Duration::from_micros(conversion_us as u64) + Duration::from_millis(REFRESH_MARGIN_MS)
}

async fn try_read_thermal(
    bus: &'static Mutex<CriticalSectionRawMutex, SmbusBus>,
) -> Result<(i16, i16), emc1403::Error<i2c::Error>> {
    let mut sensor = Emc1403::new(SharedI2c::new(bus), EMC1403_ADDR);
    sensor.probe(&mut Delay).await?;
    sensor.set_conversion_rate(EMC1403_RATE).await?;

    let internal_c = sensor.read_temp_c(Channel::Internal).await?;
    let external1_c = sensor.read_temp_c(Channel::External1).await?;

    Ok(((internal_c * 10.0) as i16, (external1_c * 10.0) as i16))
}
