//! Telemetry for the LiteOn PA-2311-02A PSU's secondary-side SMBus. Two
//! chips on this bus are confirmed and identified, and read for real via
//! their own drivers:
//! - Thermal: the chip at 0x4D is a Microchip EMC1403 (see the `emc1403`
//!   crate), so [`PsuTelemetry`]'s two temperature fields are real reads.
//! - Voltage/Current/Power: the chip at 0x40 is a TI INA219 (see the
//!   `ina219` crate), calibrated per [`INA219_CALIBRATION_RAW`]/
//!   [`INA219_CURRENT_LSB_MA`] — this board's confirmed, final shunt
//!   resistor value, not a placeholder (see that constant's doc comment).
//!
//! The PSU's own PMBus chip's register map is still undocumented and isn't
//! touched here at all — this module only ever talks to the two confirmed
//! chips above.

use defmt::warn;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::Watch;
use embassy_time::{Delay, Duration, Timer};
use emc1403::{Channel, Emc1403};
use ina219::Ina219;
use mcu_hal::i2c;

use crate::board::SmbusBus;

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

/// Bundled PA-2311-02A telemetry. Voltage (mV), Current (mA), and Power (mW)
/// are real reads from the confirmed INA219 at 0x40, calibrated per
/// [`INA219_CALIBRATION_RAW`] (see that constant's doc comment). Internal
/// Diode and External Diode 1 (tenths of a degree C)
/// are real, read from the confirmed EMC1403 at 0x4D — Internal Diode is the
/// on-die sensor in the EMC1403 package itself; External Diode 1 is
/// wherever on the PSU board its remote diode is actually soldered
/// (undocumented on this board). External Diode 2/3 aren't modeled —
/// nothing indicates they're wired on this PSU (see the `emc1403` crate's
/// device-identity doc). All fields zero until [`telemetry_task`] first runs
/// [`update_telemetry`].
#[derive(Clone, Copy, Default)]
pub struct PsuTelemetry {
    pub voltage_mv: u16,
    /// Signed — the INA219 is bidirectional, and IN+/IN− on this board could
    /// plausibly be swapped (see `INA219_register_map.md` S1).
    pub current_ma: i16,
    pub power_mw: u32,
    pub internal_decic: i16,
    pub external1_decic: i16,
}

/// Value plus change notification in one watch, so `hid::psu_report_task`
/// can push a HID Input report only when this actually changes instead of
/// polling on a timer. One receiver, for that task.
pub static PSU_TELEMETRY: Watch<CriticalSectionRawMutex, PsuTelemetry, 1> =
    Watch::new_with(PsuTelemetry {
        voltage_mv: 0,
        current_ma: 0,
        power_mw: 0,
        internal_decic: 0,
        external1_decic: 0,
    });

#[embassy_executor::task]
pub async fn telemetry_task(mut i2c: SmbusBus) -> ! {
    // Give the PSU time to power up / the bus to settle after board reset.
    Timer::after(Duration::from_secs(2)).await;

    // Written once, not every cycle: the INA219 shares this board's power
    // domain with the RP2040 (it's not on a rail that can drop out from
    // under a still-running controller), so there's no independent-reset
    // scenario for [`try_read_power_rail`] to self-heal from — matching the
    // datasheet's own init-once-per-reset model (datasheet S8.6) rather than
    // rewriting Calibration on every read.
    if let Err(e) = Ina219::new(
        &mut i2c,
        INA219_ADDR,
        INA219_CURRENT_LSB_MA,
        INA219_CALIBRATION_RAW,
    )
    .calibrate()
    .await
    {
        warn!(
            "INA219 calibration write failed: {} — current/power will read 0 until reboot",
            defmt::Debug2Format(&e)
        );
    }

    loop {
        update_telemetry(&mut i2c).await;
        Timer::after(Duration::from_secs(3)).await;
    }
}

/// Refreshes [`PSU_TELEMETRY`] from both real sensors on this bus — the
/// INA219 (voltage/current/power) via [`try_read_power_rail`] and the
/// EMC1403 (temperature) via [`try_read_thermal`]. The EMC1403 is re-probed
/// every call rather than caching an "already identified" flag, so a PSU
/// power cycle self-heals on the next pass instead of latching a failure
/// forever — see [`telemetry_task`] for why the INA219 half doesn't need
/// the same treatment. On either half failing, that half's previous cycle's
/// values are kept rather than zeroed, so a transient bus hiccup doesn't
/// look like a real 0 reading downstream.
async fn update_telemetry(i2c: &mut SmbusBus) {
    let mut telemetry = PSU_TELEMETRY.try_get().unwrap();

    match try_read_power_rail(i2c).await {
        Ok((voltage_mv, current_ma, power_mw)) => {
            telemetry.voltage_mv = voltage_mv;
            telemetry.current_ma = current_ma;
            telemetry.power_mw = power_mw;
        }
        Err(e) => warn!("INA219 power rail read failed: {}", defmt::Debug2Format(&e)),
    }

    match try_read_thermal(i2c).await {
        Ok((internal_decic, external1_decic)) => {
            telemetry.internal_decic = internal_decic;
            telemetry.external1_decic = external1_decic;
        }
        Err(e) => warn!("EMC1403 thermal read failed: {}", defmt::Debug2Format(&e)),
    }

    PSU_TELEMETRY.sender().send(telemetry);
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

/// The INA219 half of [`update_telemetry`], split out so `?` can bail on the
/// first failure without the caller needing to know which read failed.
/// Returns (voltage_mv, current_ma, power_mw). Doesn't call
/// [`Ina219::calibrate`] — [`telemetry_task`] already wrote the Calibration
/// register once at startup, and passing `INA219_CURRENT_LSB_MA` here again
/// is enough for this cycle's fresh `Ina219` wrapper (constructed fresh so
/// [`try_read_thermal`] can borrow `i2c` in between) to decode
/// Current/Power correctly without rewriting anything.
async fn try_read_power_rail(i2c: &mut SmbusBus) -> Result<(u16, i16, u32), PowerRailError> {
    let mut sensor = Ina219::new(
        i2c,
        INA219_ADDR,
        INA219_CURRENT_LSB_MA,
        INA219_CALIBRATION_RAW,
    );

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

/// The EMC1403 half of [`update_telemetry`], split out so `?` can bail on
/// the first failure — probe or either channel read — without the caller
/// needing to know which. Returns (internal, external1) in tenths of a
/// degree C.
async fn try_read_thermal(i2c: &mut SmbusBus) -> Result<(i16, i16), emc1403::Error<i2c::Error>> {
    let mut sensor = Emc1403::new(i2c, EMC1403_ADDR);
    sensor.probe(&mut Delay).await?;

    let internal_c = sensor.read_temp_c(Channel::Internal).await?;
    let external1_c = sensor.read_temp_c(Channel::External1).await?;

    Ok(((internal_c * 10.0) as i16, (external1_c * 10.0) as i16))
}
