#![no_std]
#![no_main]

mod board;
mod hid;
#[cfg(feature = "neopixel")]
mod neopixel;
mod processor_thermal;
mod pwm;
mod shared_i2c;
mod smbus;
mod storage;

use embassy_executor::Spawner;
use mcu_hal::config::Config;
use {defmt_rtt as _, panic_probe as _};

fn clock_config() -> Config {
    Config::default()
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let p = board::split();

    // --- Restore brightness saved from a previous power cycle, if any ---
    let mut store = storage::init(p.flash);
    if let Some(brightness) = storage::load(&mut store).await {
        hid::restore_brightness(brightness);
    }
    spawner.spawn(storage::task(store).unwrap());

    // --- USB HID (VESA Monitor brightness + PSU telemetry) ---
    let usb = hid::init(p.usb, p.unique_id);
    spawner.spawn(hid::usb_task(usb.usb).unwrap());
    spawner.spawn(hid::hid_report_task(usb.brightness_writer).unwrap());
    spawner.spawn(hid::power_report_task(usb.power_writer).unwrap());
    spawner.spawn(hid::power_thermal_report_task(usb.power_thermal_writer).unwrap());
    spawner.spawn(hid::processor_thermal_report_task(usb.processor_thermal_writer).unwrap());

    // --- Backlight PWM ---
    let backlight = pwm::init(p.backlight);
    spawner.spawn(pwm::task(backlight).unwrap());

    // --- SMBus telemetry for the PA-2311-02A PSU ---
    spawner.spawn(smbus::ina219_task(p.smbus).unwrap());
    spawner.spawn(smbus::emc1403_task(p.smbus).unwrap());

    // --- RP2040 on-die temperature ---
    spawner.spawn(processor_thermal::task(p.adc, p.processor_thermal_channel).unwrap());

    // --- Brightness-mirroring NeoPixel (optional, "neopixel" feature) ---
    #[cfg(feature = "neopixel")]
    spawner.spawn(neopixel::task(p.neopixel).unwrap());

    core::future::pending::<()>().await;
    unreachable!()
}
