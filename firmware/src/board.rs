//! Hardware bring-up for the current board (RP2040, Raspberry Pi Pico).
//!
//! `unique_id` reads the QSPI flash chip's factory-programmed 64-bit ID via
//! JEDEC "Read Unique ID" (command `0x4B`).

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use mcu_hal::flash::{self, Flash};
use mcu_hal::i2c::{self, I2c};
use mcu_hal::pwm::{self, Pwm};
use mcu_hal::usb::{self, Driver};
use mcu_hal::{Peri, bind_interrupts, dma, peripherals};
use static_cell::StaticCell;

use crate::clock_config;

type UsbPeripheral = peripherals::USB;
pub type UsbDriver = Driver<'static, UsbPeripheral>;

type BacklightSlice = peripherals::PWM_SLICE7;
type BacklightPin = peripherals::PIN_15;
pub type Backlight = Pwm<'static>;
const BACKLIGHT_FREQUENCY_HZ: u32 = 13_000;

type SmbusPeripheral = peripherals::I2C0;
type SmbusSclPin = peripherals::PIN_5;
type SmbusSdaPin = peripherals::PIN_4;
pub type SmbusBus = I2c<'static, SmbusPeripheral, i2c::Async>;
const SMBUS_FREQUENCY_HZ: u32 = 100_000;

type FlashPeripheral = peripherals::FLASH;
type FlashDma = peripherals::DMA_CH0;
pub const FLASH_SIZE: usize = 2 * 1024 * 1024;
pub type BoardFlash = Flash<'static, FlashPeripheral, flash::Async, FLASH_SIZE>;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<UsbPeripheral>;
    I2C0_IRQ => i2c::InterruptHandler<SmbusPeripheral>;
    DMA_IRQ_0 => dma::InterruptHandler<FlashDma>;
});

pub struct Board {
    pub usb: UsbDriver,
    pub backlight: Backlight,
    pub smbus: &'static Mutex<CriticalSectionRawMutex, SmbusBus>,
    pub flash: BoardFlash,
    pub unique_id: &'static str,
}

pub fn split() -> Board {
    let p = mcu_hal::init(clock_config());

    let mut flash = Flash::new(p.FLASH, p.DMA_CH0, Irqs);
    let mut raw_id = [0u8; 8];
    flash.blocking_unique_id(&mut raw_id).unwrap();

    static SMBUS: StaticCell<Mutex<CriticalSectionRawMutex, SmbusBus>> = StaticCell::new();

    Board {
        usb: Driver::new(p.USB, Irqs),
        backlight: backlight_pwm(p.PWM_SLICE7, p.PIN_15),
        smbus: SMBUS.init(Mutex::new(smbus_bus(p.I2C0, p.PIN_5, p.PIN_4))),
        flash,
        unique_id: hex_encode(raw_id),
    }
}

fn hex_encode(bytes: [u8; 8]) -> &'static str {
    const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

    static BUF: StaticCell<[u8; 16]> = StaticCell::new();
    let buf = BUF.init([0; 16]);
    for (byte, digits) in bytes.iter().zip(buf.as_chunks_mut::<2>().0) {
        digits[0] = HEX_DIGITS[(byte >> 4) as usize];
        digits[1] = HEX_DIGITS[(byte & 0x0F) as usize];
    }
    core::str::from_utf8(buf).unwrap()
}

fn backlight_pwm(
    slice: Peri<'static, BacklightSlice>,
    pin: Peri<'static, BacklightPin>,
) -> Backlight {
    let divider: u8 = 1;
    let top =
        (mcu_hal::clocks::clk_sys_freq() / (BACKLIGHT_FREQUENCY_HZ * divider as u32)) as u16 - 1;

    let mut config = pwm::Config::default();
    config.divider = divider.into();
    config.top = top;

    Pwm::new_output_b(slice, pin, config)
}

fn smbus_bus(
    i2c0: Peri<'static, SmbusPeripheral>,
    scl: Peri<'static, SmbusSclPin>,
    sda: Peri<'static, SmbusSdaPin>,
) -> SmbusBus {
    let mut config = i2c::Config::default();
    config.frequency = SMBUS_FREQUENCY_HZ;
    I2c::new_async(i2c0, scl, sda, Irqs, config)
}
