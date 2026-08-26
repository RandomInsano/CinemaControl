//! Backlight PWM: TIM1 CH1, 13 kHz, duty driven by the HID brightness value.

use embassy_stm32::gpio::{AfioRemap, OutputType};
use embassy_stm32::peripherals;
use embassy_stm32::time::khz;
use embassy_stm32::timer::low_level::CountingMode;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm, SimplePwmChannel};
use embassy_stm32::timer::Ch1;
use embassy_stm32::Peri;

use crate::hid;

/// Sets up TIM1 CH1 as a 13 kHz PWM output on the given pin, with its duty
/// cycle initialized from the current [`hid::BRIGHTNESS`]. Returns the
/// enabled channel, ready to be spawned via [`task`].
pub fn init(
    tim1: Peri<'static, peripherals::TIM1>,
    backlight_pin: Peri<'static, peripherals::PA8>,
) -> SimplePwmChannel<'static, peripherals::TIM1> {
    let pwm_pin: PwmPin<'_, peripherals::TIM1, Ch1, AfioRemap<0>> =
        PwmPin::new(backlight_pin, OutputType::PushPull);
    let pwm = SimplePwm::new(
        tim1,
        Some(pwm_pin),
        None,
        None,
        None,
        khz(13),
        CountingMode::EdgeAlignedUp,
    );

    let channels = pwm.split();
    let mut backlight = channels.ch1;
    backlight.enable();
    backlight.set_duty_cycle(scale_to_duty(
        hid::BRIGHTNESS.load(core::sync::atomic::Ordering::Relaxed),
        backlight.max_duty_cycle(),
    ));

    backlight
}

fn scale_to_duty(brightness: u16, max_duty: u32) -> u32 {
    ((brightness as u64 * max_duty as u64) / hid::MAX_BRIGHTNESS as u64) as u32
}

#[embassy_executor::task]
pub async fn task(mut backlight: SimplePwmChannel<'static, peripherals::TIM1>) -> ! {
    let max_duty = backlight.max_duty_cycle();
    loop {
        let v = hid::BRIGHTNESS_CHANGED.wait().await;
        backlight.set_duty_cycle(scale_to_duty(v, max_duty));
    }
}
