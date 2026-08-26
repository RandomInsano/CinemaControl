//! Peripheral/pin assignments for the current board (STM32F103C8, "Blue
//! Pill"). Every other module refers to peripherals through these aliases
//! rather than `mcu_hal::peripherals` directly, so porting to a
//! different chip or pinout means editing this file — plus the
//! `embassy-stm32` chip feature in `Cargo.toml` and the clock config in
//! `main.rs`, both of which are inherently chip-specific — and nothing else.
//!
//! Interrupt vector names bound via `bind_interrupts!` (e.g. `I2C1_EV`,
//! `DMA1_CHANNEL6`) aren't aliased here: they're PAC-defined identifiers, not
//! types, so they still need updating at each `bind_interrupts!` call site
//! when the chip changes.

use mcu_hal::peripherals;

pub type UsbPeripheral = peripherals::USB;
pub type UsbDpPin = peripherals::PA12;
pub type UsbDmPin = peripherals::PA11;

pub type BacklightTimer = peripherals::TIM1;
pub type BacklightPin = peripherals::PA8;

pub type SmbusPeripheral = peripherals::I2C1;
pub type SmbusSclPin = peripherals::PB6;
pub type SmbusSdaPin = peripherals::PB7;
pub type SmbusDmaTx = peripherals::DMA1_CH6;
pub type SmbusDmaRx = peripherals::DMA1_CH7;

pub type FlashPeripheral = peripherals::FLASH;
