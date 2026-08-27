//! Read-only SMBus diagnostic scanner for the LiteOn PA-2311-02A PSU.
//!
//! The PSU's register map isn't publicly documented, so this doesn't try to
//! be a real driver yet: it scans the bus, probes the standard PMBus command
//! set, and falls back to a raw register sweep, logging everything over
//! defmt/RTT so it can be mapped from real captures. No writes are ever
//! issued to the PSU.

use defmt::info;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::Watch;
use embassy_time::{Duration, Timer};
use embedded_hal_async::i2c::I2c as _;

use crate::board::SmbusBus;

/// Bundled PA-2311-02A telemetry: voltage (mV), current (mA), and
/// temperature (tenths of a degree C). All zero until [`scan_task`] actually
/// decodes a real PMBus reply (see the module doc comment) instead of
/// [`send_dummy_telemetry`].
#[derive(Clone, Copy, Default)]
pub struct PsuTelemetry {
    pub voltage_mv: u16,
    pub current_ma: u16,
    pub temperature_decic: i16,
}

/// Value plus change notification in one watch, so `hid::psu_report_task`
/// can push a HID Input report only when this actually changes instead of
/// polling on a timer. One receiver, for that task.
pub static PSU_TELEMETRY: Watch<CriticalSectionRawMutex, PsuTelemetry, 1> =
    Watch::new_with(PsuTelemetry {
        voltage_mv: 0,
        current_ma: 0,
        temperature_decic: 0,
    });

/// Standard PMBus command codes to probe on any address that ACKs, so we can
/// map the PA-2311-02A's (undocumented) register set from real bus captures.
const PMBUS_PROBE_COMMANDS: &[(&str, u8)] = &[
    ("STATUS_WORD", 0x79),
    ("STATUS_VOUT", 0x7A),
    ("STATUS_IOUT", 0x7B),
    ("STATUS_TEMPERATURE", 0x7D),
    ("STATUS_FANS_1_2", 0x81),
    ("READ_VIN", 0x88),
    ("READ_IIN", 0x89),
    ("READ_VOUT", 0x8B),
    ("READ_IOUT", 0x8C),
    ("READ_TEMPERATURE_1", 0x8D),
    ("READ_TEMPERATURE_2", 0x8E),
    ("READ_FAN_SPEED_1", 0x90),
    ("READ_POUT", 0x96),
    ("READ_PIN", 0x97),
    ("PMBUS_REVISION", 0x98),
    ("MFR_ID", 0x99),
    ("MFR_MODEL", 0x9A),
    ("MFR_REVISION", 0x9B),
];

#[embassy_executor::task]
pub async fn scan_task(mut i2c: SmbusBus) -> ! {
    // Give the PSU time to power up / the bus to settle after board reset.
    Timer::after(Duration::from_secs(2)).await;

    let mut dummy_cycle: u16 = 0;
    loop {
        scan_bus(&mut i2c).await;

        // The register map isn't decoded yet (see the module doc comment),
        // so there's nothing real to feed `PSU_TELEMETRY` from. Push a
        // slowly-varying made-up reading instead, purely so `psu_report_task`
        // has actual changes to exercise its change-only reporting with.
        send_dummy_telemetry(dummy_cycle);
        dummy_cycle = dummy_cycle.wrapping_add(1);

        Timer::after(Duration::from_secs(30)).await;
    }
}

/// See the call site in [`scan_task`]: not a PSU reading, just a value that
/// moves a little every cycle.
fn send_dummy_telemetry(cycle: u16) {
    PSU_TELEMETRY.sender().send(PsuTelemetry {
        voltage_mv: 12_000 + (cycle % 50) * 4,
        current_ma: 1_500 + (cycle % 30) * 10,
        temperature_decic: 350 + (cycle % 20) as i16 * 5,
    });
}

/// Runs one full pass over every 7-bit address, logging whatever is found.
async fn scan_bus(i2c: &mut SmbusBus) {
    info!("=== SMBus scan starting ===");
    for addr in 0x08u8..0x78 {
        // Cheaply check whether anything ACKs at `addr`, without assuming it
        // speaks any particular command set.
        let mut probe = [0u8; 1];
        let present = i2c.write_read(addr, &[0x00], &mut probe).await.is_ok()
            || i2c.write(addr, &[]).await.is_ok();

        if present {
            info!("device found at address 0x{:02x}", addr);
            probe_pmbus_commands(i2c, addr).await;
            raw_register_sweep(i2c, addr).await;
        }
    }
    info!("=== SMBus scan complete, sleeping ===");
}

/// Reads every standard PMBus command in [`PMBUS_PROBE_COMMANDS`] from
/// `addr` and logs whichever ones get a reply.
async fn probe_pmbus_commands(i2c: &mut SmbusBus, addr: u8) {
    for (name, cmd) in PMBUS_PROBE_COMMANDS {
        let mut buf = [0u8; 2];
        if i2c.write_read(addr, &[*cmd], &mut buf).await.is_ok() {
            info!("  0x{:02x} {}: {:02x}", cmd, name, buf);
        }
    }
}

/// Fallback for PSUs that don't speak standard PMBus commands: dumps every
/// single-byte register reply we can get from `addr`, for offline analysis.
async fn raw_register_sweep(i2c: &mut SmbusBus, addr: u8) {
    for reg in 0x00u8..=0xFF {
        let mut buf = [0u8; 1];
        if i2c.write_read(addr, &[reg], &mut buf).await.is_ok() {
            info!("  raw[0x{:02x}] = 0x{:02x}", reg, buf[0]);
        }
    }
}
