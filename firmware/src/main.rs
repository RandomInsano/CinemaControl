#![no_std]
#![no_main]

mod board;
mod hid;
mod hid_tools;
mod pwm;
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
    spawner.spawn(hid::psu_report_task(usb.psu_writer).unwrap());

    // --- Backlight PWM ---
    let backlight = pwm::init(p.backlight);
    spawner.spawn(pwm::task(backlight).unwrap());

    // --- SMBus diagnostic scanner for the PA-2311-02A PSU (read-only) ---
    spawner.spawn(smbus::scan_task(p.smbus).unwrap());

    core::future::pending::<()>().await;
    unreachable!()
}
