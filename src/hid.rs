//! USB HID: VESA/USB Monitor Control Class brightness control, plus a
//! placeholder HID Power Device (PSU telemetry) interface.

use core::sync::atomic::{AtomicI16, AtomicU16, Ordering};

use defmt::warn;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::Timer;
use embassy_usb::class::hid::{
    Config as HidConfig, HidBootProtocol, HidSubclass, HidWriter, ReportId, RequestHandler,
    State as HidState,
};
use embassy_usb::control::OutResponse;
use embassy_usb::{Builder, Config as UsbConfig, UsbDevice};
use static_cell::StaticCell;

use crate::board::UsbDriver;
use crate::hid_tools::{LoadLeBytes, Report};

/// Current backlight brightness, 0..=1023. Source of truth for both the HID
/// Feature/Input report and the PWM duty cycle.
pub static BRIGHTNESS: AtomicU16 = AtomicU16::new(512);
/// Wakes the PWM task whenever the host writes a new brightness value.
pub static BRIGHTNESS_CHANGED: Signal<CriticalSectionRawMutex, u16> = Signal::new();

pub const MAX_BRIGHTNESS: u16 = 1023;

/// Sets the startup brightness restored from flash, without signaling
/// [`BRIGHTNESS_CHANGED`] (so it isn't immediately saved straight back).
pub fn restore_brightness(value: u16) {
    BRIGHTNESS.store(value.min(MAX_BRIGHTNESS), Ordering::Relaxed);
}

/// Placeholder PA-2311-02A telemetry, exposed over HID so the wire format
/// exists before we know how to actually read it (see `src/smbus.rs`). All
/// zero until something populates them from a real PMBus read.
pub static PSU_VOLTAGE_MV: AtomicU16 = AtomicU16::new(0);
pub static PSU_CURRENT_MA: AtomicU16 = AtomicU16::new(0);
pub static PSU_TEMPERATURE_DECIC: AtomicI16 = AtomicI16::new(0);

/// VESA/USB Monitor Control Class HID report descriptor: a single Monitor
/// Control application collection (Usage Page 0x80, Usage 0x01) containing a
/// VESA Virtual Controls Brightness usage (Usage Page 0x82, Usage 0x10, VCP
/// code 0x10 "Luminance") as both an Input report (so hosts/backlight
/// drivers that only poll the interrupt IN endpoint see the live value) and
/// a Feature report (so hosts can Get/Set it directly). This mirrors the
/// structure real-world implementations (e.g. Apple Studio Display, Linux's
/// hid_bl VESA VCP backlight driver) match against.
#[rustfmt::skip]
const MONITOR_REPORT_DESCRIPTOR: &[u8] = &[
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

/// HID Power Device report descriptor (Usage Page 0x84, Usage 0x05
/// "PowerSupply" — deliberately not 0x04 "UPS", since this isn't a
/// battery-backup device and shouldn't be treated like one). No Report IDs:
/// it's the only report this interface has, so Voltage + Current +
/// Temperature just concatenate into one 6-byte Input and one 6-byte
/// Feature report. Temperature is signed tenths of a degree C (-40.0 to
/// 150.0); Voltage/Current are unsigned millivolts/milliamps.
#[rustfmt::skip]
const PSU_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x84,       // Usage Page (Power Device)
    0x09, 0x05,       // Usage (PowerSupply)
    0xA1, 0x01,       // Collection (Application)
    0x15, 0x00,       //   Logical Minimum (0)
    0x26, 0xFF, 0x7F, //   Logical Maximum (32767)
    0x75, 0x10,       //   Report Size (16)
    0x95, 0x01,       //   Report Count (1)
    0x09, 0x30,       //   Usage (Voltage)
    0x81, 0x02,       //   Input (Data,Var,Abs)
    0x09, 0x30,       //   Usage (Voltage)
    0xB1, 0x02,       //   Feature (Data,Var,Abs)
    0x09, 0x31,       //   Usage (Current)
    0x81, 0x02,       //   Input (Data,Var,Abs)
    0x09, 0x31,       //   Usage (Current)
    0xB1, 0x02,       //   Feature (Data,Var,Abs)
    0x16, 0x70, 0xFE, //   Logical Minimum (-400)
    0x26, 0xDC, 0x05, //   Logical Maximum (1500)
    0x09, 0x36,       //   Usage (Temperature)
    0x81, 0x02,       //   Input (Data,Var,Abs)
    0x09, 0x36,       //   Usage (Temperature)
    0xB1, 0x02,       //   Feature (Data,Var,Abs)
    0xC0,             // End Collection
];

struct BrightnessHandler;

impl RequestHandler for BrightnessHandler {
    fn get_report(&mut self, id: ReportId, buf: &mut [u8]) -> Option<usize> {
        match id {
            ReportId::Feature(_) | ReportId::In(_) => {
                let mut report = Report::new(buf);
                report.field(&BRIGHTNESS);
                Some(report.len())
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

/// Read-only: reports whatever's in the PSU telemetry statics, rejects
/// writes. Consistent with `src/smbus.rs` staying read-only until we
/// actually know the PA-2311-02A's register map.
struct PsuHandler;

impl RequestHandler for PsuHandler {
    fn get_report(&mut self, id: ReportId, buf: &mut [u8]) -> Option<usize> {
        match id {
            ReportId::Feature(_) | ReportId::In(_) => {
                let mut report = Report::new(buf);
                report
                    .field(&PSU_VOLTAGE_MV)
                    .field(&PSU_CURRENT_MA)
                    .field(&PSU_TEMPERATURE_DECIC);
                Some(report.len())
            }
            _ => None,
        }
    }
}

/// Everything spawned tasks need: the [`UsbDevice`] itself, and each HID
/// interface's writer for pushing Input reports.
pub struct UsbPeripherals {
    pub usb: UsbDevice<'static, UsbDriver>,
    pub brightness_writer: HidWriter<'static, UsbDriver, 2>,
    pub psu_writer: HidWriter<'static, UsbDriver, 6>,
}

/// Sets up the USB device with two HID interfaces: the VESA Monitor
/// brightness control, and a placeholder PSU telemetry interface. Ready to
/// spawn via [`usb_task`], [`hid_report_task`] and [`psu_report_task`].
pub fn init(usb_driver: UsbDriver) -> UsbPeripherals {
    let mut builder = usb_builder(usb_driver);

    static BRIGHTNESS_HANDLER: StaticCell<BrightnessHandler> = StaticCell::new();
    static BRIGHTNESS_STATE: StaticCell<HidState> = StaticCell::new();
    let brightness_writer = build_hid_writer(
        &mut builder,
        MONITOR_REPORT_DESCRIPTOR,
        BRIGHTNESS_HANDLER.init(BrightnessHandler),
        BRIGHTNESS_STATE.init(HidState::new()),
    );

    static PSU_HANDLER: StaticCell<PsuHandler> = StaticCell::new();
    static PSU_STATE: StaticCell<HidState> = StaticCell::new();
    let psu_writer = build_hid_writer(
        &mut builder,
        PSU_REPORT_DESCRIPTOR,
        PSU_HANDLER.init(PsuHandler),
        PSU_STATE.init(HidState::new()),
    );

    let usb = builder.build();

    UsbPeripherals {
        usb,
        brightness_writer,
        psu_writer,
    }
}

fn usb_device_config() -> UsbConfig<'static> {
    let mut config = UsbConfig::new(0x1209, 0xCC02); // pid.codes shared testing VID:PID
    config.manufacturer = Some("CinemaControl");
    config.product = Some("CinemaControl Monitor Controller");
    config.serial_number = Some("CC-0001");
    config.max_power = 100;
    config.max_packet_size_0 = 64;
    config.device_class = 0x00;
    config.device_sub_class = 0x00;
    config.device_protocol = 0x00;
    config.composite_with_iads = false;
    config
}

/// Allocates the descriptor/control buffers `Builder` needs (as `'static`
/// storage, since the `UsbDevice` it produces gets spawned as a task) and
/// starts a `Builder` from them.
fn usb_builder(usb_driver: UsbDriver) -> Builder<'static, UsbDriver> {
    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static MSOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

    Builder::new(
        usb_driver,
        usb_device_config(),
        CONFIG_DESC.init([0; 256]),
        BOS_DESC.init([0; 256]),
        MSOS_DESC.init([0; 256]),
        CONTROL_BUF.init([0; 64]),
    )
}

/// Registers one HID interface on `builder` and returns its writer for
/// pushing Input reports.
fn build_hid_writer<const N: usize>(
    builder: &mut Builder<'static, UsbDriver>,
    report_descriptor: &'static [u8],
    request_handler: &'static mut dyn RequestHandler,
    state: &'static mut HidState<'static>,
) -> HidWriter<'static, UsbDriver, N> {
    let config = HidConfig {
        report_descriptor,
        request_handler: Some(request_handler),
        poll_ms: 60,
        max_packet_size: 8,
        hid_subclass: HidSubclass::No,
        hid_boot_protocol: HidBootProtocol::None,
    };
    HidWriter::new(builder, state, config)
}

#[embassy_executor::task]
pub async fn usb_task(mut usb: UsbDevice<'static, UsbDriver>) -> ! {
    usb.run().await
}

#[embassy_executor::task]
pub async fn hid_report_task(mut writer: HidWriter<'static, UsbDriver, 2>) -> ! {
    writer.ready().await;
    loop {
        if let Err(e) = writer.write(&BRIGHTNESS.load_le_bytes()).await {
            warn!("hid input report write failed: {:?}", e);
        }
        Timer::after_millis(200).await;
    }
}

#[embassy_executor::task]
pub async fn psu_report_task(mut writer: HidWriter<'static, UsbDriver, 6>) -> ! {
    writer.ready().await;
    loop {
        let mut report_buf = [0u8; 6];
        Report::new(&mut report_buf)
            .field(&PSU_VOLTAGE_MV)
            .field(&PSU_CURRENT_MA)
            .field(&PSU_TEMPERATURE_DECIC);
        if let Err(e) = writer.write(&report_buf).await {
            warn!("psu input report write failed: {:?}", e);
        }
        Timer::after_millis(1000).await;
    }
}
