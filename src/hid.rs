//! USB HID: VESA/USB Monitor Control Class brightness control.

use core::sync::atomic::{AtomicU16, Ordering};

use defmt::warn;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::peripherals;
use embassy_stm32::usb::{self, Driver};
use embassy_stm32::{bind_interrupts, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::Timer;
use embassy_usb::class::hid::{
    Config as HidConfig, HidBootProtocol, HidSubclass, HidWriter, ReportId, RequestHandler, State as HidState,
};
use embassy_usb::control::OutResponse;
use embassy_usb::{Builder, Config as UsbConfig, UsbDevice};
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    USB_LP_CAN1_RX0 => usb::InterruptHandler<peripherals::USB>;
});

/// Current backlight brightness, 0..=1023. Source of truth for both the HID
/// Feature/Input report and the PWM duty cycle.
pub static BRIGHTNESS: AtomicU16 = AtomicU16::new(512);
/// Wakes the PWM task whenever the host writes a new brightness value.
pub static BRIGHTNESS_CHANGED: Signal<CriticalSectionRawMutex, u16> = Signal::new();

pub const MAX_BRIGHTNESS: u16 = 1023;

/// VESA/USB Monitor Control Class HID report descriptor: a single Monitor
/// Control application collection (Usage Page 0x80, Usage 0x01) containing a
/// VESA Virtual Controls Brightness usage (Usage Page 0x82, Usage 0x10, VCP
/// code 0x10 "Luminance") as both an Input report (so hosts/backlight
/// drivers that only poll the interrupt IN endpoint see the live value) and
/// a Feature report (so hosts can Get/Set it directly). This mirrors the
/// structure real-world implementations (e.g. Apple Studio Display, Linux's
/// hid_bl VESA VCP backlight driver) match against.
#[rustfmt::skip]
const HID_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x80,       // Usage Page (Monitor)
    0x09, 0x01,       // Usage (Monitor Control)
    0xA1, 0x01,       // Collection (Application)
    0x05, 0x82,       //   Usage Page (VESA Virtual Controls)
    0x09, 0x10,       //   Usage (Brightness / VCP 0x10)
    0x15, 0x00,       //   Logical Minimum (0)
    0x26, 0xFF, 0x03, //   Logical Maximum (1023)
    0x75, 0x10,       //   Report Size (16)
    0x95, 0x01,       //   Report Count (1)
    0x81, 0x02,       //   Input (Data,Var,Abs)
    0x09, 0x10,       //   Usage (Brightness / VCP 0x10)
    0xB1, 0x02,       //   Feature (Data,Var,Abs)
    0xC0,             // End Collection
];

struct BrightnessHandler;

impl RequestHandler for BrightnessHandler {
    fn get_report(&mut self, id: ReportId, buf: &mut [u8]) -> Option<usize> {
        match id {
            ReportId::Feature(_) | ReportId::In(_) => {
                let v = BRIGHTNESS.load(Ordering::Relaxed);
                buf[0..2].copy_from_slice(&v.to_le_bytes());
                Some(2)
            }
            _ => None,
        }
    }

    fn set_report(&mut self, id: ReportId, data: &[u8]) -> OutResponse {
        match id {
            ReportId::Feature(_) if data.len() >= 2 => {
                let v = u16::from_le_bytes([data[0], data[1]]).min(MAX_BRIGHTNESS);
                BRIGHTNESS.store(v, Ordering::Relaxed);
                BRIGHTNESS_CHANGED.signal(v);
                defmt::info!("brightness set to {}", v);
                OutResponse::Accepted
            }
            _ => OutResponse::Rejected,
        }
    }
}

/// Concrete USB driver type for this board, so task signatures elsewhere
/// don't need to spell it out.
pub type UsbDriver = Driver<'static, peripherals::USB>;

/// Sets up the USB HID VESA Monitor interface. Returns the built
/// [`UsbDevice`] and its HID writer, ready to be spawned as tasks via
/// [`usb_task`] and [`hid_report_task`].
pub async fn init(
    usb: Peri<'static, peripherals::USB>,
    mut dp: Peri<'static, peripherals::PA12>,
    dm: Peri<'static, peripherals::PA11>,
) -> (UsbDevice<'static, UsbDriver>, HidWriter<'static, UsbDriver, 2>) {
    // Blue Pill quirk: PA12 (USB D+) has a fixed external 1.5k pull-up, so
    // the host may not notice a reset/re-flash as a fresh enumeration. Force
    // a bus disconnect by briefly driving D+ low ourselves before handing
    // the pin to the USB peripheral.
    {
        let _dp = Output::new(dp.reborrow(), Level::Low, Speed::Low);
        Timer::after_millis(10).await;
    }

    let usb_driver = Driver::new(usb, Irqs, dp, dm);

    let mut usb_config = UsbConfig::new(0x1209, 0xCC02); // pid.codes shared testing VID:PID
    usb_config.manufacturer = Some("CinemaControl");
    usb_config.product = Some("CinemaControl Monitor Controller");
    usb_config.serial_number = Some("CC-0001");
    usb_config.max_power = 100;
    usb_config.max_packet_size_0 = 64;
    usb_config.device_class = 0x00;
    usb_config.device_sub_class = 0x00;
    usb_config.device_protocol = 0x00;
    usb_config.composite_with_iads = false;

    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static MSOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    static HID_STATE: StaticCell<HidState> = StaticCell::new();
    static HANDLER: StaticCell<BrightnessHandler> = StaticCell::new();

    let handler = HANDLER.init(BrightnessHandler);

    let mut builder = Builder::new(
        usb_driver,
        usb_config,
        CONFIG_DESC.init([0; 256]),
        BOS_DESC.init([0; 256]),
        MSOS_DESC.init([0; 256]),
        CONTROL_BUF.init([0; 64]),
    );

    let hid_config = HidConfig {
        report_descriptor: HID_REPORT_DESCRIPTOR,
        request_handler: Some(handler),
        poll_ms: 60,
        max_packet_size: 8,
        hid_subclass: HidSubclass::No,
        hid_boot_protocol: HidBootProtocol::None,
    };
    let hid_writer: HidWriter<'static, UsbDriver, 2> =
        HidWriter::new(&mut builder, HID_STATE.init(HidState::new()), hid_config);

    let usb = builder.build();

    (usb, hid_writer)
}

#[embassy_executor::task]
pub async fn usb_task(mut usb: UsbDevice<'static, UsbDriver>) -> ! {
    usb.run().await
}

#[embassy_executor::task]
pub async fn hid_report_task(mut writer: HidWriter<'static, UsbDriver, 2>) -> ! {
    writer.ready().await;
    loop {
        let v = BRIGHTNESS.load(Ordering::Relaxed);
        if let Err(e) = writer.write(&v.to_le_bytes()).await {
            warn!("hid input report write failed: {:?}", e);
        }
        Timer::after_millis(200).await;
    }
}
