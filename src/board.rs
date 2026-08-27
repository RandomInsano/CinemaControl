//! Peripheral/pin assignments for the current board (RP2040, Raspberry Pi
//! Pico). Every other module — including `main.rs` — refers to peripherals
//! through the aliases, [`Irqs`] and [`split`] below rather than
//! `mcu_hal::peripherals` / `mcu_hal::Peripherals` fields directly, so
//! porting to a different chip or pinout means editing this file — plus the
//! `embassy-rp` chip feature in `Cargo.toml` and the clock config in
//! `main.rs`, both of which are inherently chip-specific — and nothing else.
//!
//! [`Irqs`] binds every interrupt vector this board uses in one place, so
//! the PAC-defined vector names (`USBCTRL_IRQ`, `I2C0_IRQ`, ...) — spelled
//! exactly as the chip's vendor SVD names them, not something we can alias —
//! only need updating here when the chip changes, not at each peripheral
//! module's call site.
//!
//! Each peripheral module's `init()` takes one resource struct (e.g.
//! [`BacklightResources`]) rather than loose `Peri` arguments, so adding or
//! dropping a pin for that peripheral only means editing the struct here —
//! not every call site's parameter list.

use mcu_hal::{Peri, bind_interrupts, dma, i2c, peripherals, usb};

use crate::clock_config;

pub type UsbPeripheral = peripherals::USB;

pub type BacklightSlice = peripherals::PWM_SLICE7;
pub type BacklightPin = peripherals::PIN_15;

pub type SmbusPeripheral = peripherals::I2C0;
pub type SmbusSclPin = peripherals::PIN_5;
pub type SmbusSdaPin = peripherals::PIN_4;

pub type FlashPeripheral = peripherals::FLASH;
pub type FlashDma = peripherals::DMA_CH0;

bind_interrupts!(pub struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<UsbPeripheral>;
    I2C0_IRQ => i2c::InterruptHandler<SmbusPeripheral>;
    DMA_IRQ_0 => dma::InterruptHandler<FlashDma>;
});

pub struct Board {
    pub usb: Peri<'static, UsbPeripheral>,
    pub backlight: BacklightResources,
    pub smbus: SmbusResources,
    pub flash: FlashResources,
}

pub struct BacklightResources {
    pub slice: Peri<'static, BacklightSlice>,
    pub pin: Peri<'static, BacklightPin>,
}

pub struct SmbusResources {
    pub i2c: Peri<'static, SmbusPeripheral>,
    pub scl: Peri<'static, SmbusSclPin>,
    pub sda: Peri<'static, SmbusSdaPin>,
}

pub struct FlashResources {
    pub flash: Peri<'static, FlashPeripheral>,
    pub dma: Peri<'static, FlashDma>,
}

pub fn split() -> Board {
    let p = mcu_hal::init(clock_config());

    Board {
        usb: p.USB,
        backlight: BacklightResources {
            slice: p.PWM_SLICE7,
            pin: p.PIN_15,
        },
        smbus: SmbusResources {
            i2c: p.I2C0,
            scl: p.PIN_5,
            sda: p.PIN_4,
        },
        flash: FlashResources {
            flash: p.FLASH,
            dma: p.DMA_CH0,
        },
    }
}
