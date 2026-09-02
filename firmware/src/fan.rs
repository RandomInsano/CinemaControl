//! PID-ramped fan speed with a tach fail-safe, held against the PSU's
//! external1 thermal probe (`smbus.rs`'s [`POWER_THERMAL_TELEMETRY`]). See
//! `fan-control` for the control loop itself; this module only wires it up
//! to the board's PWM output + tach input (`board.rs`).

use core::convert::Infallible;
use core::time::Duration as StdDuration;

use defmt::warn;
use embassy_time::{Duration, Instant, Timer};
use embedded_fans_async::{ErrorType, Fan, RpmSense};
use fan_control::{FanController, FanState};
use mcu_hal::pwm::SetDutyCycle;

use crate::board::{FanPwmOutput, FanTachInput};
use crate::smbus::POWER_THERMAL_TELEMETRY;

/// PID goal, held against `PowerThermalTelemetry::external1_decic`.
const GOAL_TEMP_C: f32 = 40.0;
const KP: f32 = 4.0;
const KI: f32 = 0.5;
const KD: f32 = 0.5;

// Specs for the Delta Electronics BFB1012MD
const MAX_RPM: u16 = 3200;
const MIN_START_RPM: u16 = 600;
/// Comfortably below idle RPM so ordinary spin-up doesn't trip the fail-safe.
const MIN_PLAUSIBLE_RPM: u16 = 300;

/// Long enough to clear a normal spin-up without eating too much of the
/// loop's ~5s thermal reaction budget.
const TACH_FAIL_TIMEOUT: StdDuration = StdDuration::from_secs(3);
const TICK: Duration = Duration::from_millis(500);

/// PC-style fans emit two tach pulses per shaft revolution (Intel's 4-Wire
/// PWM Fan spec, S3).
const TACH_PULSES_PER_REV: u32 = 2;

struct BoardFan {
    pwm: FanPwmOutput,
    tach: FanTachInput,
    last_counter: u16,
    last_sample: Instant,
}

impl BoardFan {
    fn new(mut pwm: FanPwmOutput, tach: FanTachInput) -> Self {
        pwm.set_duty_cycle(0).unwrap();
        let last_counter = tach.counter();
        Self {
            pwm,
            tach,
            last_counter,
            last_sample: Instant::now(),
        }
    }
}

impl ErrorType for BoardFan {
    type Error = Infallible;
}

impl Fan for BoardFan {
    fn max_rpm(&self) -> u16 {
        MAX_RPM
    }

    fn min_rpm(&self) -> u16 {
        0
    }

    fn min_start_rpm(&self) -> u16 {
        MIN_START_RPM
    }

    async fn set_speed_rpm(&mut self, rpm: u16) -> Result<u16, Infallible> {
        let rpm = rpm.min(MAX_RPM);
        let max_duty = self.pwm.max_duty_cycle();
        let duty = ((u32::from(rpm) * u32::from(max_duty)) / u32::from(MAX_RPM)) as u16;
        self.pwm.set_duty_cycle(duty).unwrap();
        Ok(rpm)
    }
}

impl RpmSense for BoardFan {
    async fn rpm(&mut self) -> Result<u16, Infallible> {
        let now = Instant::now();
        let elapsed_ms = now.duration_since(self.last_sample).as_millis().max(1);

        let counter = self.tach.counter();
        let pulses = u32::from(counter.wrapping_sub(self.last_counter));
        self.last_counter = counter;
        self.last_sample = now;

        let revolutions = pulses / TACH_PULSES_PER_REV;
        let rpm = u64::from(revolutions) * 60_000 / elapsed_ms;
        Ok(rpm.min(u64::from(u16::MAX)) as u16)
    }
}

#[embassy_executor::task]
pub async fn task(pwm: FanPwmOutput, tach: FanTachInput) -> ! {
    let fan = BoardFan::new(pwm, tach);
    let mut controller = FanController::new(
        fan,
        GOAL_TEMP_C,
        KP,
        KI,
        KD,
        MIN_PLAUSIBLE_RPM,
        TACH_FAIL_TIMEOUT,
    );

    let mut thermal = POWER_THERMAL_TELEMETRY.receiver().unwrap();
    let mut last_state = None;
    let dt = StdDuration::from_millis(TICK.as_millis());

    loop {
        let temp_c = f32::from(thermal.get().await.external1_decic) / 10.0;

        let state = controller.update(dt, temp_c).await.unwrap();
        if last_state != Some(state) && state == FanState::Failed {
            warn!("Fan tachometer not confirming rotation — failing fully-on");
        }
        last_state = Some(state);

        Timer::after(TICK).await;
    }
}
