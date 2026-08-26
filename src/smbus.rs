//! Read-only SMBus diagnostic scanner for the LiteOn PA-2311-02A PSU.
//!
//! The PSU's register map isn't publicly documented, so this doesn't try to
//! be a real driver yet: it scans the bus, probes the standard PMBus command
//! set, and falls back to a raw register sweep, logging everything over
//! defmt/RTT so it can be mapped from real captures. No writes are ever
//! issued to the PSU.

use defmt::info;
use embassy_stm32::i2c::{self, I2c};
use embassy_stm32::mode::Async;
use embassy_stm32::peripherals;
use embassy_stm32::{bind_interrupts, Peri};
use embassy_time::{Duration, Timer};

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    DMA1_CHANNEL6 => embassy_stm32::dma::InterruptHandler<peripherals::DMA1_CH6>;
    DMA1_CHANNEL7 => embassy_stm32::dma::InterruptHandler<peripherals::DMA1_CH7>;
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

/// Sets up I2C1 for the SMBus diagnostic scanner, ready to be spawned via
/// [`scan_task`].
pub fn init(
    i2c1: Peri<'static, peripherals::I2C1>,
    scl: Peri<'static, peripherals::PB6>,
    sda: Peri<'static, peripherals::PB7>,
    dma_tx: Peri<'static, peripherals::DMA1_CH6>,
    dma_rx: Peri<'static, peripherals::DMA1_CH7>,
) -> I2c<'static, Async, i2c::Master> {
    let mut config = i2c::Config::default();
    config.frequency = embassy_stm32::time::khz(100);
    I2c::new(i2c1, scl, sda, dma_tx, dma_rx, Irqs, config)
}

#[embassy_executor::task]
pub async fn scan_task(mut i2c: I2c<'static, Async, i2c::Master>) -> ! {
    // Give the PSU time to power up / the bus to settle after board reset.
    Timer::after(Duration::from_secs(2)).await;

    loop {
        info!("=== SMBus scan starting ===");
        for addr in 0x08u8..0x78 {
            let mut probe = [0u8; 1];
            let present = i2c.write_read(addr, &[0x00], &mut probe).await.is_ok()
                || i2c.write(addr, &[]).await.is_ok();
            if present {
                info!("device found at address 0x{:02x}", addr);

                for (name, cmd) in PMBUS_PROBE_COMMANDS {
                    let mut buf = [0u8; 2];
                    if i2c.write_read(addr, &[*cmd], &mut buf).await.is_ok() {
                        info!("  0x{:02x} {}: {:02x}", cmd, name, buf);
                    }
                }

                // Raw sweep fallback in case the PSU doesn't speak standard
                // PMBus commands at all: dump every single-byte register
                // reply we can get, for offline analysis.
                for reg in 0x00u8..=0xFF {
                    let mut buf = [0u8; 1];
                    if i2c.write_read(addr, &[reg], &mut buf).await.is_ok() {
                        info!("  raw[0x{:02x}] = 0x{:02x}", reg, buf[0]);
                    }
                }
            }
        }
        info!("=== SMBus scan complete, sleeping ===");
        Timer::after(Duration::from_secs(30)).await;
    }
}
