//! Telemetry for the LiteOn PA-2311-02A PSU's secondary-side SMBus. Two
//! chips on this bus are confirmed and identified, and read for real via
//! their own drivers, each on its own independent task since their ADCs
//! have very different sample latency (see [`ina219_task`]/
//! [`emc1403_task`]):
//! - Thermal: the chip at 0x4D is a Microchip EMC1403 (see the `emc1403`
//!   crate), so [`ThermalTelemetry`]'s two fields are real reads.
//! - Voltage/Current/Power: the chip at 0x40 is a TI INA219 (see the
//!   `ina219` crate), calibrated per [`INA219_CALIBRATION_RAW`]/
//!   [`INA219_CURRENT_LSB_MA`] — this board's confirmed, final shunt
//!   resistor value, not a placeholder (see that constant's doc comment).
//!
//! Both tasks share the one physical bus via `board::Board::smbus`/
//! [`crate::shared_i2c::SharedI2c`], but each owns its own [`Watch`]
//! ([`POWER_TELEMETRY`], [`THERMAL_TELEMETRY`]) — no shared state between
//! them means no risk of one clobbering the other's update, and each only
//! sends when its own reading actually changed from the last one it sent.
//!
//! The PSU's own PMBus chip's register map is still undocumented and isn't
//! touched here at all — this module only ever talks to the two confirmed
//! chips above.

use defmt::warn;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::watch::Watch;
use embassy_time::{Delay, Duration, Timer};
use emc1403::{Channel, ConversionRate, Emc1403};
use ina219::Ina219;
use mcu_hal::i2c;

use crate::board::SmbusBus;
use crate::shared_i2c::SharedI2c;

/// ADC precision [`ina219_task`] actually configures the INA219 to use —
/// the most averaging the chip supports (smoothest, lowest-noise readings),
/// chosen because even its worst-case conversion time (~136ms, doubled for
/// BADC+SADC — see [`ina219::AdcSetting::conversion_time_us`]) still leaves
/// [`ina219_task`]'s refresh cadence comfortably fast for a live display.
const INA219_PRECISION: ina219::AdcSetting = ina219::AdcSetting::Average128;

/// Rate [`emc1403_task`] actually configures the EMC1403 to convert at —
/// confirmed against the datasheet as this chip's default (4 conversions/
/// sec, 250ms period).
const EMC1403_RATE: ConversionRate = ConversionRate::PerSec4;

/// Extra headroom [`refresh_interval_for`] adds on top of a device's raw
/// conversion time, so ordinary task-scheduling jitter can't have a task
/// poll a conversion that's a few microseconds from done but not quite
/// there yet.
const REFRESH_MARGIN_MS: u64 = 14;

/// The shortest interval safe to poll a device without re-reading a
/// still-in-progress conversion, plus [`REFRESH_MARGIN_MS`]. `conversion_us`
/// is the device's own full-cycle conversion time — see
/// [`ina219_task`]/[`emc1403_task`] for how each derives theirs.
fn refresh_interval_for(conversion_us: u32) -> Duration {
    Duration::from_micros(conversion_us as u64) + Duration::from_millis(REFRESH_MARGIN_MS)
}

/// Confirmed physical device: an EMC1403-2 on the PSU's secondary-side
/// SMBus (see the `emc1403` crate's device-identity doc) — only the
/// internal diode and External Diode 1 are wired on this board.
const EMC1403_ADDR: u8 = emc1403::address::EMC1403_2_EMC1404_2;

/// Confirmed physical device: a TI INA219 on the PSU's secondary-side SMBus,
/// both A0/A1 strapped to GND (see `INA219_register_map.md`, S2).
const INA219_ADDR: u8 = ina219::Address::GndGnd as u8;

/// Confirmed final calibration for this board's INA219 (`INA219_register_map.md`
/// S9) — the shunt resistor was confirmed at 2.5mOhm. 1mA/bit was already
/// the correct rounded `Current_LSB` even under the earlier placeholder
/// resistor guess: 25.8A expected max (~27% headroom over this rail's
/// nameplate rating) / 32767 ~= 787uA minimum, rounded up to the nearest
/// whole milliamp per S9. `Cal = trunc(0.04096 / (0.001A * 0.0025ohm)) =
/// 16384` (0x4000). `Power_LSB = 20 * Current_LSB = 20mW`.
const INA219_CURRENT_LSB_MA: u32 = 1;
const INA219_CALIBRATION_RAW: u16 = 0x4000;

/// Real reads from the confirmed INA219 at 0x40, calibrated per
/// [`INA219_CALIBRATION_RAW`] (see that constant's doc comment). Voltage in
/// mV, Current in mA (signed — the INA219 is bidirectional, and IN+/IN− on
/// this board could plausibly be swapped, see `INA219_register_map.md` S1),
/// Power in mW. Zero until [`ina219_task`] first sends a real reading.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct PowerTelemetry {
    pub voltage_mv: u16,
    pub current_ma: i16,
    pub power_mw: u32,
}

/// Value plus change notification in one watch, so `hid::power_report_task`
/// can push a HID Input report only when this actually changes instead of
/// polling on a timer. [`ina219_task`] only sends here when a new reading
/// actually differs from the last one it sent — see its doc comment.
pub static POWER_TELEMETRY: Watch<CriticalSectionRawMutex, PowerTelemetry, 1> =
    Watch::new_with(PowerTelemetry {
        voltage_mv: 0,
        current_ma: 0,
        power_mw: 0,
    });

/// Real reads from the confirmed EMC1403 at 0x4D, in tenths of a degree C —
/// Internal Diode is the on-die sensor in the EMC1403 package itself;
/// External Diode 1 is wherever on the PSU board its remote diode is
/// actually soldered (undocumented on this board). External Diode 2/3
/// aren't modeled — nothing indicates they're wired on this PSU (see the
/// `emc1403` crate's device-identity doc). Zero until [`emc1403_task`]
/// first sends a real reading.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ThermalTelemetry {
    pub internal_decic: i16,
    pub external1_decic: i16,
}

/// Same shape as [`POWER_TELEMETRY`], for [`emc1403_task`].
pub static THERMAL_TELEMETRY: Watch<CriticalSectionRawMutex, ThermalTelemetry, 1> =
    Watch::new_with(ThermalTelemetry {
        internal_decic: 0,
        external1_decic: 0,
    });

#[embassy_executor::task]
pub async fn ina219_task(bus: &'static Mutex<CriticalSectionRawMutex, SmbusBus>) -> ! {
    // Give the PSU time to power up / the bus to settle after board reset.
    Timer::after(Duration::from_secs(2)).await;

    // Held for this task's entire lifetime rather than reconstructed per
    // cycle: `SharedI2c` is a cheap, `Copy` handle onto the shared bus, so
    // holding one doesn't block `emc1403_task` from using the bus too (see
    // `shared_i2c.rs`) — unlike a bare `&mut SmbusBus`, which is why this
    // used to have to be rebuilt every poll.
    let mut sensor = Ina219::new(
        SharedI2c::new(bus),
        INA219_ADDR,
        INA219_CURRENT_LSB_MA,
        INA219_CALIBRATION_RAW,
    );

    // The INA219 shares this board's power domain with the RP2040 (it's
    // not on a rail that can drop out from under a still-running
    // controller), so there's no independent-reset scenario to self-heal
    // from — Configuration/Calibration only need setting once, at startup,
    // matching the datasheet's own init-once-per-reset model (S8.6) rather
    // than rewriting them every read.
    let config = ina219::Configuration {
        bus_adc: INA219_PRECISION,
        shunt_adc: INA219_PRECISION,
        ..ina219::Configuration::default()
    };
    if let Err(e) = sensor.set_configuration(config).await {
        warn!(
            "INA219 set_configuration failed: {} — falling back to its power-on default precision",
            defmt::Debug2Format(&e)
        );
    }
    if let Err(e) = sensor.calibrate().await {
        warn!(
            "INA219 calibration write failed: {} — current/power will read 0 until reboot",
            defmt::Debug2Format(&e)
        );
    }

    // Doubled for BADC+SADC — Configuration.MODE here is shunt+bus
    // continuous, which converts both every cycle (datasheet S4.1: total
    // conversion time per cycle is roughly BADC time + SADC time, not the
    // max of either alone).
    let refresh_interval = refresh_interval_for(2 * INA219_PRECISION.conversion_time_us());

    // Tracked locally rather than re-reading `POWER_TELEMETRY.try_get()`
    // each cycle: this task is the only writer, so a local copy is enough
    // to compare against, and it means a genuinely unchanged reading never
    // touches the watch at all — see the module doc comment.
    let mut last_sent = PowerTelemetry::default();

    loop {
        match try_read_power_rail(&mut sensor).await {
            Ok((voltage_mv, current_ma, power_mw)) => {
                let telemetry = PowerTelemetry {
                    voltage_mv,
                    current_ma,
                    power_mw,
                };
                if telemetry != last_sent {
                    POWER_TELEMETRY.sender().send(telemetry);
                    last_sent = telemetry;
                }
            }
            Err(e) => warn!("INA219 power rail read failed: {}", defmt::Debug2Format(&e)),
        }
        Timer::after(refresh_interval).await;
    }
}

/// [`try_read_power_rail`]'s error type: either a bus/driver error, or a
/// successful-but-untrustworthy read (`ina219::BusVoltage::overflow` set —
/// would mean the load exceeded [`INA219_CALIBRATION_RAW`]'s calibrated
/// range, e.g. a transient well past the ~27% headroom it was sized for).
#[derive(Debug)]
enum PowerRailError {
    Bus(ina219::Error<i2c::Error>),
    Overflow,
}

impl From<ina219::Error<i2c::Error>> for PowerRailError {
    fn from(e: ina219::Error<i2c::Error>) -> Self {
        Self::Bus(e)
    }
}

/// Returns (voltage_mv, current_ma, power_mw).
async fn try_read_power_rail(
    sensor: &mut Ina219<SharedI2c>,
) -> Result<(u16, i16, u32), PowerRailError> {
    let voltage = sensor.bus_voltage().await?;
    if voltage.overflow {
        return Err(PowerRailError::Overflow);
    }

    // Current_LSB is 1mA/bit here, so this cast is exact — current_ma()'s
    // i32 only exists to accommodate a larger Current_LSB than this driver
    // currently uses.
    let current_ma = sensor.current_ma().await? as i16;
    let power_mw = sensor.power_mw().await?;

    Ok((voltage.millivolts, current_ma, power_mw))
}

#[embassy_executor::task]
pub async fn emc1403_task(bus: &'static Mutex<CriticalSectionRawMutex, SmbusBus>) -> ! {
    // Give the PSU time to power up / the bus to settle after board reset.
    Timer::after(Duration::from_secs(2)).await;

    let refresh_interval = refresh_interval_for(EMC1403_RATE.period_us());

    // See the identical comment in `ina219_task` — this task is the only
    // writer to `THERMAL_TELEMETRY`, so a local copy is enough to compare
    // against.
    let mut last_sent = ThermalTelemetry::default();

    loop {
        match try_read_thermal(bus).await {
            Ok((internal_decic, external1_decic)) => {
                let telemetry = ThermalTelemetry {
                    internal_decic,
                    external1_decic,
                };
                if telemetry != last_sent {
                    THERMAL_TELEMETRY.sender().send(telemetry);
                    last_sent = telemetry;
                }
            }
            Err(e) => warn!("EMC1403 thermal read failed: {}", defmt::Debug2Format(&e)),
        }
        Timer::after(refresh_interval).await;
    }
}

/// Split out so `?` can bail on the first failure — probe, conversion-rate
/// write, or either channel read — without the caller needing to know
/// which. Returns (internal, external1) in tenths of a degree C.
///
/// Re-probed and re-configured every call rather than caching an "already
/// set up" flag, unlike the INA219 (see [`ina219_task`]): the EMC1403 has
/// no such power-domain guarantee documented, so this self-heals on the
/// next pass instead of latching a failure forever if it's ever reset
/// independently of the RP2040.
async fn try_read_thermal(
    bus: &'static Mutex<CriticalSectionRawMutex, SmbusBus>,
) -> Result<(i16, i16), emc1403::Error<i2c::Error>> {
    let mut sensor = Emc1403::new(SharedI2c::new(bus), EMC1403_ADDR);
    sensor.probe(&mut Delay).await?;
    sensor.set_conversion_rate(EMC1403_RATE).await?;

    let internal_c = sensor.read_temp_c(Channel::Internal).await?;
    let external1_c = sensor.read_temp_c(Channel::External1).await?;

    Ok(((internal_c * 10.0) as i16, (external1_c * 10.0) as i16))
}
