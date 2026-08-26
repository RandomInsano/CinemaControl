//! Backlight PWM: TIM1 CH1, 13 kHz, duty driven by the HID brightness value.

use core::sync::atomic::Ordering;

use mcu_hal::Peri;
use mcu_hal::gpio::{AfioRemap, OutputType};
use mcu_hal::time::khz;
use mcu_hal::timer::Ch1;
use mcu_hal::timer::low_level::CountingMode;
use mcu_hal::timer::simple_pwm::{PwmPin, SimplePwm, SimplePwmChannel};

use crate::board::{BacklightPin, BacklightTimer};
use crate::hid;

type Backlight = SimplePwmChannel<'static, BacklightTimer>;

/// Sets up TIM1 CH1 as a 13 kHz PWM output on the given pin, with its duty
/// cycle initialized from the current [`hid::BRIGHTNESS`]. Returns the
/// enabled channel, ready to be spawned via [`task`].
pub fn init(
    tim1: Peri<'static, BacklightTimer>,
    backlight_pin: Peri<'static, BacklightPin>,
) -> Backlight {
    let pwm = create_pwm(tim1, backlight_pin);

    let mut backlight = pwm.split().ch1;
    backlight.enable();
    let brightness = hid::BRIGHTNESS.load(Ordering::Relaxed);
    backlight.set_duty_cycle(scale_to_duty(brightness, backlight.max_duty_cycle()));

    backlight
}

fn create_pwm(
    tim1: Peri<'static, BacklightTimer>,
    backlight_pin: Peri<'static, BacklightPin>,
) -> SimplePwm<'static, BacklightTimer> {
    let pwm_pin: PwmPin<'_, BacklightTimer, Ch1, AfioRemap<0>> =
        PwmPin::new(backlight_pin, OutputType::PushPull);
    SimplePwm::new(
        tim1,
        Some(pwm_pin),
        None,
        None,
        None,
        khz(13),
        CountingMode::EdgeAlignedUp,
    )
}

fn scale_to_duty(brightness: u16, max_duty: u32) -> u32 {
    ((brightness as u64 * max_duty as u64) / hid::MAX_BRIGHTNESS as u64) as u32
}

#[embassy_executor::task]
pub async fn task(mut backlight: Backlight) -> ! {
    let max_duty = backlight.max_duty_cycle();
    loop {
        let v = hid::BRIGHTNESS_CHANGED.wait().await;
        backlight.set_duty_cycle(scale_to_duty(v, max_duty));
    }
}
