//! Backlight PWM: PWM slice 7 channel B, 13 kHz, duty driven by the HID
//! brightness value.

use core::sync::atomic::Ordering;

use mcu_hal::pwm::{Config, Pwm, SetDutyCycle};

use crate::board::BacklightResources;
use crate::hid;

type Backlight = Pwm<'static>;

const FREQUENCY_HZ: u32 = 13_000;

/// Sets up the board's backlight slice/pin as a 13 kHz PWM output, with its
/// duty cycle initialized from the current [`hid::BRIGHTNESS`]. Ready to be
/// spawned via [`task`].
pub fn init(resources: BacklightResources) -> Backlight {
    let mut backlight = create_pwm(resources);

    let brightness = hid::BRIGHTNESS.load(Ordering::Relaxed);
    backlight
        .set_duty_cycle(scale_to_duty(brightness, backlight.max_duty_cycle()))
        .unwrap();

    backlight
}

fn create_pwm(resources: BacklightResources) -> Backlight {
    let divider: u8 = 1;
    let top = (mcu_hal::clocks::clk_sys_freq() / (FREQUENCY_HZ * divider as u32)) as u16 - 1;

    let mut config = Config::default();
    config.divider = divider.into();
    config.top = top;

    Pwm::new_output_b(resources.slice, resources.pin, config)
}

fn scale_to_duty(brightness: u16, max_duty: u16) -> u16 {
    ((brightness as u32 * max_duty as u32) / hid::MAX_BRIGHTNESS as u32) as u16
}

#[embassy_executor::task]
pub async fn task(mut backlight: Backlight) -> ! {
    let max_duty = backlight.max_duty_cycle();
    loop {
        let v = hid::BRIGHTNESS_CHANGED.wait().await;
        backlight.set_duty_cycle(scale_to_duty(v, max_duty)).unwrap();
    }
}
