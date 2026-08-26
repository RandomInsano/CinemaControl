#![no_std]
#![no_main]

mod hid;
mod pwm;
mod smbus;
mod storage;

use embassy_executor::Spawner;
use embassy_stm32::rcc::{Hse, HseMode, Pll, PllMul, PllPreDiv, PllSource, Sysclk};
use embassy_stm32::Config;
use {defmt_rtt as _, panic_probe as _};

fn clock_config() -> Config {
    let mut config = Config::default();
    config.rcc.hse = Some(Hse {
        freq: embassy_stm32::time::mhz(8),
        mode: HseMode::Oscillator,
    });
    config.rcc.pll = Some(Pll {
        src: PllSource::HSE,
        prediv: PllPreDiv::DIV1,
        mul: PllMul::MUL9,
    });
    config.rcc.sys = Sysclk::PLL1_P;
    config
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let p = embassy_stm32::init(clock_config());

    // --- Restore brightness saved from a previous power cycle, if any ---
    let mut store = storage::init(p.FLASH);
    if let Some(brightness) = storage::load(&mut store).await {
        hid::restore_brightness(brightness);
    }
    spawner.spawn(storage::task(store).unwrap());

    // --- USB HID (VESA Monitor brightness + PSU telemetry) ---
    let usb = hid::init(p.USB, p.PA12, p.PA11).await;
    spawner.spawn(hid::usb_task(usb.usb).unwrap());
    spawner.spawn(hid::hid_report_task(usb.brightness_writer).unwrap());
    spawner.spawn(hid::psu_report_task(usb.psu_writer).unwrap());

    // --- Backlight PWM: TIM1 CH1 / PA8, 13 kHz ---
    let backlight = pwm::init(p.TIM1, p.PA8);
    spawner.spawn(pwm::task(backlight).unwrap());

    // --- SMBus diagnostic scanner for the PA-2311-02A PSU (read-only) ---
    let i2c = smbus::init(p.I2C1, p.PB6, p.PB7, p.DMA1_CH6, p.DMA1_CH7);
    spawner.spawn(smbus::scan_task(i2c).unwrap());

    // main() has nothing left to do; park it.
    core::future::pending::<()>().await;
    unreachable!()
}
