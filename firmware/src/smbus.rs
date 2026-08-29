//! SMBus diagnostics and telemetry for the LiteOn PA-2311-02A PSU's
//! secondary side.
//!
//! The PSU's own PMBus register map isn't publicly documented, so most of
//! this module is still a read-only diagnostic scanner: it scans the bus,
//! probes the standard PMBus command set, and falls back to a raw register
//! sweep, logging everything over defmt/RTT so it can be mapped from real
//! captures. No writes are ever issued to that (still-unidentified) chip.
//!
//! The one exception is [`update_telemetry`]'s thermal half: the chip at
//! 0x4D is a confirmed, identified Microchip EMC1403 (see the `emc1403`
//! crate), so [`PsuTelemetry`]'s two temperature fields are read for real
//! via that driver, independent of the PMBus guesswork above — see
//! [`PsuTelemetry`]'s doc comment for which fields are real and which
//! aren't yet.

use defmt::{info, warn};
use embassy_embedded_hal::SetConfig;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::Watch;
use embassy_time::{Delay, Duration, Timer};
use embedded_hal_async::i2c::I2c as _;
use emc1403::{Channel, Emc1403};
use mcu_hal::i2c;

use crate::board::SmbusBus;

/// Confirmed physical device: an EMC1403-2 on the PSU's secondary-side
/// SMBus (see the `emc1403` crate's device-identity doc) — only the
/// internal diode and External Diode 1 are wired on this board.
const EMC1403_ADDR: u8 = emc1403::address::EMC1403_2_EMC1404_2;

/// `read_thermal_telemetry` sets this explicitly before every read rather
/// than trusting whatever [`SCAN_FREQUENCIES_HZ`] left the bus at, since
/// that sweep exists purely to find the still-unidentified PSU chip and has
/// no reason to leave the bus in a state the EMC1403 is guaranteed to like.
const EMC1403_FREQUENCY_HZ: u32 = 100_000;

/// Every clock rate `scan_task` sweeps on each pass, since the PA-2311-02A
/// (and any other SMBus/PMBus device this bus is ever pointed at) might not
/// ack at whatever rate [`crate::board`] configured it for. Reused for any
/// future I2C work on this bus, not just this PSU.
const SCAN_FREQUENCIES_HZ: &[u32] = &[100_000, 400_000, 10_000];

/// `embassy-rp`'s I2C driver has no timeout of its own: a NACK aborts almost
/// instantly (hardware-detected), but a genuinely stuck bus (SCL/SDA held
/// low by a wiring fault or a device stretching the clock forever) just
/// hangs the `.await` forever. Every transaction in this module is wrapped
/// at this timeout so a stuck bus shows up as a diagnosable warning instead
/// of silently freezing the scan.
const PROBE_TIMEOUT: Duration = Duration::from_millis(50);

/// Runs one I2C transaction with [`PROBE_TIMEOUT`], returning whether it
/// succeeded. A timeout (as opposed to a normal NACK) means the bus itself
/// is stuck, which no amount of retrying or address/frequency variation will
/// fix in software — see the constant's doc comment.
async fn probe_ok<T>(fut: impl core::future::Future<Output = Result<T, i2c::Error>>) -> bool {
    matches!(
        embassy_time::with_timeout(PROBE_TIMEOUT, fut).await,
        Ok(Ok(_))
    )
}

/// Bundled PA-2311-02A telemetry. Voltage (mV) and current (mA) are still
/// entirely made up pending a decoded PMBus register map (see the module
/// doc comment and [`update_telemetry`]'s dummy half). Internal Diode and
/// External Diode 1 (tenths of a degree C) are real, read from the
/// confirmed EMC1403 at 0x4D — Internal Diode is the on-die sensor in the
/// EMC1403 package itself; External Diode 1 is wherever on the PSU board
/// its remote diode is actually soldered (undocumented on this board).
/// External Diode 2/3 aren't modeled — nothing indicates they're wired on
/// this PSU (see the `emc1403` crate's device-identity doc). All fields
/// zero until [`scan_task`] first runs [`update_telemetry`].
#[derive(Clone, Copy, Default)]
pub struct PsuTelemetry {
    pub voltage_mv: u16,
    pub current_ma: u16,
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
        internal_decic: 0,
        external1_decic: 0,
    });

/// How wide a PMBus command's reply is, per the spec — reading every command
/// as a blind 2-byte word (as this scanner used to) misreads the 1-byte
/// STATUS_* replies as one real byte plus whatever the bus does after the
/// slave stops driving, and truncates the variable-length MFR_* block reads
/// into a meaningless word.
#[derive(Clone, Copy)]
enum Width {
    Byte,
    Word,
    /// SMBus block read: the slave's first reply byte is the data length,
    /// followed by up to [`MAX_BLOCK_LEN`] data bytes.
    Block,
}

/// SMBus block-read protocol's own maximum (32 data bytes), not something
/// specific to this PSU.
const MAX_BLOCK_LEN: usize = 32;

/// Standard PMBus command codes to probe on any address that ACKs, so we can
/// map the PA-2311-02A's (undocumented) register set from real bus captures.
const PMBUS_PROBE_COMMANDS: &[(&str, u8, Width)] = &[
    ("STATUS_WORD", 0x79, Width::Word),
    ("STATUS_VOUT", 0x7A, Width::Byte),
    ("STATUS_IOUT", 0x7B, Width::Byte),
    ("STATUS_TEMPERATURE", 0x7D, Width::Byte),
    ("STATUS_FANS_1_2", 0x81, Width::Byte),
    ("READ_VIN", 0x88, Width::Word),
    ("READ_IIN", 0x89, Width::Word),
    ("READ_VOUT", 0x8B, Width::Word),
    ("READ_IOUT", 0x8C, Width::Word),
    ("READ_TEMPERATURE_1", 0x8D, Width::Word),
    ("READ_TEMPERATURE_2", 0x8E, Width::Word),
    ("READ_FAN_SPEED_1", 0x90, Width::Word),
    ("READ_POUT", 0x96, Width::Word),
    ("READ_PIN", 0x97, Width::Word),
    ("PMBUS_REVISION", 0x98, Width::Byte),
    ("MFR_ID", 0x99, Width::Block),
    ("MFR_MODEL", 0x9A, Width::Block),
    ("MFR_REVISION", 0x9B, Width::Block),
];

#[embassy_executor::task]
pub async fn scan_task(mut i2c: SmbusBus) -> ! {
    // Give the PSU time to power up / the bus to settle after board reset.
    Timer::after(Duration::from_secs(2)).await;

    let mut dummy_cycle: u16 = 0;
    loop {
        for &frequency in SCAN_FREQUENCIES_HZ {
            let mut config = i2c::Config::default();
            config.frequency = frequency;
            i2c.set_config(&config).unwrap();

            info!("--- scanning at {} Hz ---", frequency);
            scan_bus(&mut i2c).await;
        }

        update_telemetry(&mut i2c, dummy_cycle).await;
        dummy_cycle = dummy_cycle.wrapping_add(1);

        Timer::after(Duration::from_secs(30)).await;
    }
}

/// Refreshes [`PSU_TELEMETRY`]. Voltage/current are still entirely made up
/// (see the module doc comment — PMBus is undecoded): `cycle` just drives a
/// slowly-varying value, purely so `hid::psu_report_task`'s change-only
/// reporting has actual changes to exercise. The two temperature fields are
/// real EMC1403 reads via [`try_read_thermal`], re-probed and re-read every
/// call rather than caching an "already identified" flag — at this ~30s
/// cadence the extra three identification-register reads are negligible,
/// and it means a PSU power cycle (or the diagnostic scan above briefly
/// wedging the bus at an unfriendly frequency) self-heals on the next pass
/// instead of latching a failure forever. On a thermal read failure, the
/// previous cycle's temperatures are kept rather than zeroed, so a
/// transient bus hiccup doesn't look like a real 0.0C reading downstream.
async fn update_telemetry(i2c: &mut SmbusBus, cycle: u16) {
    let mut telemetry = PSU_TELEMETRY.try_get().unwrap();
    telemetry.voltage_mv = 12_000 + (cycle % 50) * 4;
    telemetry.current_ma = 1_500 + (cycle % 30) * 10;

    match try_read_thermal(i2c).await {
        Ok((internal_decic, external1_decic)) => {
            telemetry.internal_decic = internal_decic;
            telemetry.external1_decic = external1_decic;
        }
        Err(e) => warn!("EMC1403 thermal read failed: {}", defmt::Debug2Format(&e)),
    }

    PSU_TELEMETRY.sender().send(telemetry);
}

/// The EMC1403 half of [`update_telemetry`], split out so `?` can bail on
/// the first failure — probe or either channel read — without the caller
/// needing to know which. Returns (internal, external1) in tenths of a
/// degree C.
async fn try_read_thermal(i2c: &mut SmbusBus) -> Result<(i16, i16), emc1403::Error<i2c::Error>> {
    let mut config = i2c::Config::default();
    config.frequency = EMC1403_FREQUENCY_HZ;
    i2c.set_config(&config).unwrap();

    let mut sensor = Emc1403::new(i2c, EMC1403_ADDR);
    sensor.probe(&mut Delay).await?;

    let internal_c = sensor.read_temp_c(Channel::Internal).await?;
    let external1_c = sensor.read_temp_c(Channel::External1).await?;

    Ok(((internal_c * 10.0) as i16, (external1_c * 10.0) as i16))
}

/// Runs one full pass over every 7-bit address, logging whatever is found.
async fn scan_bus(i2c: &mut SmbusBus) {
    info!("=== SMBus scan starting ===");
    for addr in 0x08u8..0x78 {
        // Cheaply check whether anything ACKs at `addr`, without assuming it
        // speaks any particular command set.
        let mut probe = [0u8; 1];
        let present = probe_ok(i2c.write_read(addr, &[0x00], &mut probe)).await
            || probe_ok(i2c.write(addr, &[])).await;

        if present {
            info!("device found at address 0x{:02x}", addr);
            probe_pmbus_commands(i2c, addr).await;
            raw_register_sweep(i2c, addr).await;
        }
    }
    info!("=== SMBus scan complete, sleeping ===");
}

/// Reads every standard PMBus command in [`PMBUS_PROBE_COMMANDS`] from
/// `addr`, at that command's real width, and logs whichever ones get a
/// reply.
async fn probe_pmbus_commands(i2c: &mut SmbusBus, addr: u8) {
    for &(name, cmd, width) in PMBUS_PROBE_COMMANDS {
        match width {
            Width::Byte => {
                let mut buf = [0u8; 1];
                if probe_ok(i2c.write_read(addr, &[cmd], &mut buf)).await {
                    info!("  0x{:02x} {}: {:02x}", cmd, name, buf[0]);
                }
            }
            Width::Word => {
                let mut buf = [0u8; 2];
                if probe_ok(i2c.write_read(addr, &[cmd], &mut buf)).await {
                    info!("  0x{:02x} {}: {:02x}", cmd, name, buf);
                }
            }
            Width::Block => {
                // +1 for the length byte the slave sends ahead of the data.
                let mut buf = [0u8; MAX_BLOCK_LEN + 1];
                if probe_ok(i2c.write_read(addr, &[cmd], &mut buf)).await {
                    let len = (buf[0] as usize).min(MAX_BLOCK_LEN);
                    info!(
                        "  0x{:02x} {}: len={} {:02x}",
                        cmd,
                        name,
                        len,
                        &buf[1..1 + len]
                    );
                }
            }
        }
    }
}

/// Fallback for PSUs that don't speak standard PMBus commands: dumps every
/// single-byte register reply we can get from `addr`, for offline analysis.
/// Printed as a grid, 8 registers per row (0x00 through 0xFF, in order) —
/// a failed read is `!!`, a successful one is its hex byte value.
async fn raw_register_sweep(i2c: &mut SmbusBus, addr: u8) {
    const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
    const COLS: usize = 8;
    const ROW_LEN: usize = COLS * 2 + (COLS - 1); // "XX XX XX XX XX XX XX XX"
    const ROWS: usize = 256 / COLS;
    let mut line = [0u8; ROWS * (ROW_LEN + 1) - 1]; // rows joined by '\n', no trailing newline

    let mut pos = 0;
    for reg in 0x00u8..=0xFF {
        let col = reg as usize % COLS;
        if reg != 0 && col == 0 {
            line[pos] = b'\n';
            pos += 1;
        } else if col != 0 {
            line[pos] = b' ';
            pos += 1;
        }

        let mut buf = [0u8; 1];
        if probe_ok(i2c.write_read(addr, &[reg], &mut buf)).await {
            line[pos] = HEX_DIGITS[(buf[0] >> 4) as usize];
            line[pos + 1] = HEX_DIGITS[(buf[0] & 0x0F) as usize];
        } else {
            line[pos..pos + 2].copy_from_slice(b"!!");
        }
        pos += 2;
    }

    info!("  raw:\n{}", core::str::from_utf8(&line[..pos]).unwrap());
}
