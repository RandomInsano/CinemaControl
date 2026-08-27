//! Backlight PWM: duty driven by the HID brightness value. Frequency/pin
//! bring-up lives in `board.rs`; this module only owns the duty cycle.

use core::sync::atomic::Ordering;

use mcu_hal::pwm::SetDutyCycle;

use crate::board::Backlight;
use crate::hid;

/// Seeds the board's backlight PWM with the current [`hid::BRIGHTNESS`].
/// Ready to be spawned via [`task`].
pub fn init(mut backlight: Backlight) -> Backlight {
    let brightness = hid::BRIGHTNESS.load(Ordering::Relaxed);
    backlight
        .set_duty_cycle(scale_to_duty(brightness, backlight.max_duty_cycle()))
        .unwrap();

    backlight
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
