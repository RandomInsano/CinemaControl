//! Hardware bring-up for the current board: a Raspberry Pi Pico or a
//! Waveshare RP2040-Zero — both plain RP2040 and wired identically for
//! everything this firmware touches, so no board-select feature exists. The
//! optional `neopixel` feature (see `neopixel.rs`) drives a WS2812 on
//! GPIO16: the RP2040-Zero's onboard one, or an external one wired up on a
//! Pico.
//!
//! `unique_id` reads the QSPI flash chip's factory-programmed 64-bit ID via
//! JEDEC "Read Unique ID" (command `0x4B`).

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use mcu_hal::adc::{self, Adc};
use mcu_hal::flash::{self, Flash};
use mcu_hal::gpio::Pull;
use mcu_hal::i2c::{self, I2c};
use mcu_hal::pwm::{self, Pwm};
use mcu_hal::usb::{self, Driver};
use mcu_hal::{Peri, bind_interrupts, dma, peripherals};
use static_cell::StaticCell;

#[cfg(feature = "neopixel")]
use mcu_hal::pio::{self, Pio};
#[cfg(feature = "neopixel")]
use mcu_hal::pio_programs::ws2812::{Grb, PioWs2812, PioWs2812Program};

use crate::clock_config;

type UsbPeripheral = peripherals::USB;
pub type UsbDriver = Driver<'static, UsbPeripheral>;

type BacklightSlice = peripherals::PWM_SLICE7;
type BacklightPin = peripherals::PIN_15;
pub type Backlight = Pwm<'static>;
const BACKLIGHT_FREQUENCY_HZ: u32 = 13_000;

// The fan's tach line needs its own PWM slice from the one driving its PWM
// output: `Pwm::new_input`'s edge-counting mode reclocks the *whole* slice's
// counter off the input pin, which would break PWM generation on that same
// slice's other channel. GPIO6/9 land on different slices (3 and 4) for
// exactly that reason — see `fan.rs`.
type FanPwmSlice = peripherals::PWM_SLICE3;
type FanPwmPin = peripherals::PIN_6;
pub type FanPwmOutput = Pwm<'static>;
const FAN_PWM_FREQUENCY_HZ: u32 = 25_000; // Intel 4-Wire PWM Fan spec, S2.1

type FanTachSlice = peripherals::PWM_SLICE4;
type FanTachPin = peripherals::PIN_9;
pub type FanTachInput = Pwm<'static>;

type SmbusPeripheral = peripherals::I2C0;
type SmbusSclPin = peripherals::PIN_5;
type SmbusSdaPin = peripherals::PIN_4;
pub type SmbusBus = I2c<'static, SmbusPeripheral, i2c::Async>;
const SMBUS_FREQUENCY_HZ: u32 = 100_000;

type FlashPeripheral = peripherals::FLASH;
type FlashDma = peripherals::DMA_CH0;
pub const FLASH_SIZE: usize = 2 * 1024 * 1024;
pub type BoardFlash = Flash<'static, FlashPeripheral, flash::Async, FLASH_SIZE>;

pub type ProcessorThermalAdc = Adc<'static, adc::Async>;
pub type ProcessorThermalChannel = adc::Channel<'static>;

#[cfg(feature = "neopixel")]
type NeopixelPeripheral = peripherals::PIO0;
#[cfg(feature = "neopixel")]
type NeopixelDma = peripherals::DMA_CH1;
#[cfg(feature = "neopixel")]
type NeopixelPin = peripherals::PIN_16;
#[cfg(feature = "neopixel")]
pub type Neopixel = PioWs2812<'static, NeopixelPeripheral, 0, 1, Grb>;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<UsbPeripheral>;
    I2C0_IRQ => i2c::InterruptHandler<SmbusPeripheral>;
    ADC_IRQ_FIFO => adc::InterruptHandler;
    DMA_IRQ_0 => dma::InterruptHandler<FlashDma>, #[cfg(feature = "neopixel")] dma::InterruptHandler<NeopixelDma>;
    #[cfg(feature = "neopixel")]
    PIO0_IRQ_0 => pio::InterruptHandler<NeopixelPeripheral>;
});

pub struct Board {
    pub usb: UsbDriver,
    pub backlight: Backlight,
    pub fan_pwm: FanPwmOutput,
    pub fan_tach: FanTachInput,
    pub smbus: &'static Mutex<CriticalSectionRawMutex, SmbusBus>,
    pub flash: BoardFlash,
    pub adc: ProcessorThermalAdc,
    pub processor_thermal_channel: ProcessorThermalChannel,
    #[cfg(feature = "neopixel")]
    pub neopixel: Neopixel,
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
        fan_pwm: fan_pwm(p.PWM_SLICE3, p.PIN_6),
        fan_tach: fan_tach(p.PWM_SLICE4, p.PIN_9),
        smbus: SMBUS.init(Mutex::new(smbus_bus(p.I2C0, p.PIN_5, p.PIN_4))),
        flash,
        adc: Adc::new(p.ADC, Irqs, adc::Config::default()),
        processor_thermal_channel: adc::Channel::new_temp_sensor(p.ADC_TEMP_SENSOR),
        #[cfg(feature = "neopixel")]
        neopixel: neopixel_ws2812(p.PIO0, p.DMA_CH1, p.PIN_16),
        unique_id: hex_encode(raw_id),
    }
}

#[cfg(feature = "neopixel")]
fn neopixel_ws2812(
    pio0: Peri<'static, NeopixelPeripheral>,
    dma: Peri<'static, NeopixelDma>,
    pin: Peri<'static, NeopixelPin>,
) -> Neopixel {
    let Pio {
        mut common, sm0, ..
    } = Pio::new(pio0, Irqs);
    let program = PioWs2812Program::new(&mut common);
    PioWs2812::new(&mut common, sm0, dma, Irqs, pin, &program)
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

fn fan_pwm(slice: Peri<'static, FanPwmSlice>, pin: Peri<'static, FanPwmPin>) -> FanPwmOutput {
    let divider: u8 = 1;
    let top =
        (mcu_hal::clocks::clk_sys_freq() / (FAN_PWM_FREQUENCY_HZ * divider as u32)) as u16 - 1;

    let mut config = pwm::Config::default();
    config.divider = divider.into();
    config.top = top;

    Pwm::new_output_a(slice, pin, config)
}

fn fan_tach(slice: Peri<'static, FanTachSlice>, pin: Peri<'static, FanTachPin>) -> FanTachInput {
    // Free-runs `top` (0xFFFF, `Config::default`) counts per tach edge;
    // `fan.rs` samples it periodically and diffs against the last reading
    // rather than wrapping it into an interrupt.
    Pwm::new_input(
        slice,
        pin,
        Pull::Up,
        pwm::InputMode::RisingEdge,
        pwm::Config::default(),
    )
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
