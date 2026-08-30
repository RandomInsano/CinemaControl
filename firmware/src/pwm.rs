//! Backlight PWM: duty driven by the HID brightness value. Frequency/pin
//! bring-up lives in `board.rs`; this module only owns the duty cycle.

use mcu_hal::pwm::SetDutyCycle;

use crate::board::Backlight;
use crate::hid;

pub fn init(mut backlight: Backlight) -> Backlight {
    set_brightness(&mut backlight, hid::BRIGHTNESS.try_get().unwrap());
    backlight
}

fn set_brightness(backlight: &mut Backlight, brightness: u16) {
    let max_duty = backlight.max_duty_cycle();
    let duty = ((brightness as u32 * max_duty as u32) / hid::MAX_BRIGHTNESS as u32) as u16;
    backlight.set_duty_cycle(duty).unwrap();
}

#[embassy_executor::task]
pub async fn task(mut backlight: Backlight) -> ! {
    let mut brightness = hid::BRIGHTNESS.receiver().unwrap();
    loop {
        let v = brightness.changed().await;
        set_brightness(&mut backlight, v);
    }
}
