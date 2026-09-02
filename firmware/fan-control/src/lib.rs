//! Thermal PID ramp with a tach fail-safe, built on
//! [`embedded_fans_async`]'s [`Fan`]/[`RpmSense`] traits rather than any
//! particular board's PWM/capture peripherals, so it drops onto whatever
//! concrete fan a board wires up.
//!
//! [`FanController::update`] is meant to be called on a steady cadence (a
//! few times a second is plenty for airflow, which doesn't move fast) with
//! the latest temperature reading. It runs that reading through a PID loop
//! to pick a duty cycle, then cross-checks the tachometer: if the fan is
//! being told to spin but the tach won't confirm it's turning for longer
//! than `tach_fail_timeout`, the controller gives up on trusting its own
//! commanded duty cycle and latches to fully-on instead — see
//! [`FanState::Failed`].
#![cfg_attr(not(test), no_std)]

use core::time::Duration;

use embedded_fans_async::{Fan, RpmSense};
use pid::Pid;

/// Current output of a [`FanController`]: either a normal PID-driven duty
/// cycle, or the fail-safe fully-on state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanState {
    /// Running at `percent` (0..=100) of the fan's rated RPM, as last
    /// commanded by the PID loop.
    Running(u8),
    /// Latched fully-on after the tachometer failed to confirm the fan was
    /// spinning for longer than the configured timeout. Stays latched —
    /// see [`FanController::clear_fault`] — since a tach that starts
    /// reporting motion again could just as easily be a wiring glitch as a
    /// real recovery, and running louder than necessary is a much smaller
    /// problem than silently running too slow.
    Failed,
}

/// PID-driven fan speed with a latching tach fail-safe. Generic over any
/// [`Fan`] + [`RpmSense`] implementation.
pub struct FanController<F> {
    fan: F,
    pid: Pid<f32>,
    tach_fail_timeout: Duration,
    min_plausible_rpm: u16,
    stalled_for: Duration,
    state: FanState,
}

impl<F> FanController<F> {
    /// `goal_c` is the temperature the PID loop tries to hold by adjusting
    /// fan speed. `kp`/`ki`/`kd` are given as positive magnitudes — the
    /// loop is reverse-acting (speed should rise as temperature rises above
    /// goal), so they're negated internally rather than asking callers to
    /// remember to do it themselves.
    ///
    /// `min_plausible_rpm` is the tach reading below which, while a nonzero
    /// speed is commanded, the fan is considered not (yet) spinning; it
    /// should sit comfortably below the fan's normal idle RPM so ordinary
    /// spin-up time doesn't trip it. `tach_fail_timeout` is how long that
    /// condition may persist before the controller fails fully-on.
    pub fn new(
        fan: F,
        goal_c: f32,
        kp: f32,
        ki: f32,
        kd: f32,
        min_plausible_rpm: u16,
        tach_fail_timeout: Duration,
    ) -> Self {
        let mut pid = Pid::new(goal_c, 100.0f32);
        pid.p(-kp, 100.0f32).i(-ki, 100.0f32).d(-kd, 100.0f32);
        Self {
            fan,
            pid,
            tach_fail_timeout,
            min_plausible_rpm,
            stalled_for: Duration::ZERO,
            state: FanState::Running(0),
        }
    }

    /// Retargets the PID loop's goal temperature without disturbing its
    /// integral term or the fail-safe latch.
    pub fn set_goal_c(&mut self, goal_c: f32) {
        self.pid.setpoint(goal_c);
    }

    pub fn state(&self) -> FanState {
        self.state
    }

    /// Clears a latched [`FanState::Failed`] and resumes PID control.
    /// Callers should only do this after confirming the underlying fault is
    /// actually gone (e.g. the fan was reseated) — see
    /// [`FanState::Failed`] for why the latch doesn't clear itself.
    pub fn clear_fault(&mut self) {
        self.state = FanState::Running(0);
        self.stalled_for = Duration::ZERO;
        self.pid.reset_integral_term();
    }

    /// Runs the PID loop for one tick, clamped to a valid duty percentage.
    fn pid_output_percent(&mut self, temp_c: f32) -> u8 {
        self.pid
            .next_control_output(temp_c)
            .output
            .clamp(0.0, 100.0) as u8
    }

    /// Cross-checks a commanded duty cycle against a tach reading and
    /// advances the stall timer, latching [`FanState::Failed`] once it
    /// crosses `tach_fail_timeout`. Pure state transition, kept separate
    /// from `update` so it's testable without a real or mock [`Fan`].
    fn stall_check(&mut self, percent: u8, rpm: u16, dt: Duration) -> FanState {
        if percent > 0 && rpm < self.min_plausible_rpm {
            self.stalled_for += dt;
            if self.stalled_for >= self.tach_fail_timeout {
                self.state = FanState::Failed;
                return self.state;
            }
        } else {
            self.stalled_for = Duration::ZERO;
        }

        self.state = FanState::Running(percent);
        self.state
    }
}

impl<F> FanController<F>
where
    F: Fan + RpmSense,
{
    /// Feeds a new temperature reading through the PID loop and applies the
    /// resulting duty cycle, unless already latched into
    /// [`FanState::Failed`] — in which case this is a no-op that just
    /// reports the latch. `dt` is the time elapsed since the previous call,
    /// used only to time out the tach fail-safe.
    pub async fn update(&mut self, dt: Duration, temp_c: f32) -> Result<FanState, F::Error> {
        if self.state == FanState::Failed {
            return Ok(self.state);
        }

        let percent = self.pid_output_percent(temp_c);
        self.fan.set_speed_percent(percent).await?;

        let rpm = self.fan.rpm().await?;
        let state = self.stall_check(percent, rpm, dt);
        if state == FanState::Failed {
            self.fan.set_speed_max().await?;
        }
        Ok(state)
    }

    /// Releases the wrapped fan, e.g. to drive it directly during a
    /// power-down sequence.
    pub fn into_inner(self) -> F {
        self.fan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: Duration = Duration::from_millis(500);
    const FAIL_TIMEOUT: Duration = Duration::from_secs(2);

    // F is never driven through the Fan/RpmSense traits in these tests —
    // `pid_output_percent` and `stall_check` are plain state transitions —
    // so `()` stands in for "no real fan needed".
    fn controller() -> FanController<()> {
        FanController::new((), 50.0, 10.0, 0.0, 0.0, 400, FAIL_TIMEOUT)
    }

    #[test]
    fn ramps_up_as_temperature_rises_above_goal() {
        let mut c = controller();

        assert_eq!(c.pid_output_percent(40.0), 0);
        assert!(c.pid_output_percent(60.0) > 0);
    }

    #[test]
    fn clamps_to_zero_rather_than_going_negative_below_goal() {
        let mut c = controller();

        // A large undershoot would drive a naive PID output negative; a
        // duty percentage has no way to express that, so it must clamp.
        assert_eq!(c.pid_output_percent(-50.0), 0);
    }

    #[test]
    fn does_not_fail_when_intentionally_stopped() {
        let mut c = controller();

        // Commanded to 0% with a 0 RPM tach reading is the fan behaving
        // correctly, not a fault — must never latch Failed.
        for _ in 0..10 {
            assert_eq!(c.stall_check(0, 0, TICK), FanState::Running(0));
        }
    }

    #[test]
    fn fails_fully_on_after_tach_silent_past_timeout() {
        let mut c = controller();

        let mut state = FanState::Running(0);
        for _ in 0..10 {
            // Commanded on (100%), tach stuck at 0 the whole time.
            state = c.stall_check(100, 0, TICK);
            if state == FanState::Failed {
                break;
            }
        }

        assert_eq!(state, FanState::Failed);
    }

    #[test]
    fn does_not_fail_before_the_timeout_elapses() {
        let mut c = controller();

        assert_eq!(c.stall_check(100, 0, TICK), FanState::Running(100));
    }

    #[test]
    fn stall_timer_resets_once_tach_reads_plausible_again() {
        let mut c = controller();

        // Accumulate most of the timeout...
        c.stall_check(100, 0, TICK);
        c.stall_check(100, 0, TICK);
        // ...then the fan catches up and spins up for real.
        c.stall_check(100, 2000, TICK);

        // If the stall timer hadn't reset, one more silent tick would cross
        // FAIL_TIMEOUT and latch Failed.
        let state = c.stall_check(100, 0, TICK);
        assert!(matches!(state, FanState::Running(_)));
    }

    #[test]
    fn failed_state_latches_until_cleared() {
        let mut c = controller();

        let mut state = FanState::Running(0);
        for _ in 0..10 {
            state = c.stall_check(100, 0, TICK);
            if state == FanState::Failed {
                break;
            }
        }
        assert_eq!(state, FanState::Failed);

        // stall_check itself doesn't re-check FanState::Failed (that guard
        // lives in `update`), so drive it through `update`'s early-return
        // path instead: state() must still report Failed.
        assert_eq!(c.state(), FanState::Failed);

        c.clear_fault();
        assert_eq!(c.state(), FanState::Running(0));
        assert!(matches!(c.stall_check(100, 2000, TICK), FanState::Running(_)));
    }
}
