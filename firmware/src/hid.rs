//! USB HID: VESA/USB Monitor Control Class brightness control, plus two
//! separate Power Device interfaces for PSU telemetry (voltage/current/power,
//! temperature).

use defmt::warn;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::Watch;
use embassy_usb::class::hid::{
    Config as HidConfig, HidBootProtocol, HidSubclass, HidWriter, ReportId, RequestHandler,
    State as HidState,
};
use embassy_usb::control::OutResponse;
use embassy_usb::{Builder, Config as UsbConfig, UsbDevice};
use static_cell::StaticCell;

use crate::board::UsbDriver;
use crate::hid_tools::Report;
use crate::smbus::{POWER_TELEMETRY, THERMAL_TELEMETRY};

pub static BRIGHTNESS: Watch<CriticalSectionRawMutex, u16, 3> = Watch::new_with(512);

pub const MAX_BRIGHTNESS: u16 = 1023;

pub fn restore_brightness(value: u16) {
    BRIGHTNESS.sender().send(value.min(MAX_BRIGHTNESS));
}

/// VESA VCP code 0x10 "Luminance", Usage Page 0x82.
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

/// HID Power Device, Usage Page 0x84, Usage 0x05 "PowerSupply".
#[rustfmt::skip]
const POWER_REPORT_DESCRIPTOR: &[u8] = &[
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

    0x16, 0x00, 0x80, //   Logical Minimum (-32768)
    0x26, 0xFF, 0x7F, //   Logical Maximum (32767)
    0x09, 0x31,       //   Usage (Current)
    0x81, 0x02,       //   Input (Data,Var,Abs)
    0x09, 0x31,       //   Usage (Current)
    0xB1, 0x02,       //   Feature (Data,Var,Abs)

    0x15, 0x00,                   //   Logical Minimum (0)
    0x27, 0x40, 0x42, 0x0F, 0x00, //   Logical Maximum (1,000,000)
    0x75, 0x20,                   //   Report Size (32)
    0x09, 0x34,                   //   Usage (ActivePower)
    0x81, 0x02,                   //   Input (Data,Var,Abs)
    0x09, 0x34,                   //   Usage (ActivePower)
    0xB1, 0x02,                   //   Feature (Data,Var,Abs)

    0xC0,             // End Collection
];

/// HID Power Device, Usage Page 0x84, Usage 0x05 "PowerSupply".
#[rustfmt::skip]
const THERMAL_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x84,       // Usage Page (Power Device)
    0x09, 0x05,       // Usage (PowerSupply)
    0xA1, 0x01,       // Collection (Application)

    0x75, 0x10,       //   Report Size (16)
    0x95, 0x02,       //   Report Count (2)
    0x16, 0x70, 0xFE, //   Logical Minimum (-400)
    0x26, 0xDC, 0x05, //   Logical Maximum (1500)
    0x09, 0x36,       //   Usage (Temperature) -- Internal Diode
    0x09, 0x36,       //   Usage (Temperature) -- External Diode 1
    0x81, 0x02,       //   Input (Data,Var,Abs)
    0x09, 0x36,       //   Usage (Temperature) -- Internal Diode
    0x09, 0x36,       //   Usage (Temperature) -- External Diode 1
    0xB1, 0x02,       //   Feature (Data,Var,Abs)

    0xC0,             // End Collection
];

struct BrightnessHandler;

impl RequestHandler for BrightnessHandler {
    fn get_report(&mut self, id: ReportId, buf: &mut [u8]) -> Option<usize> {
        match id {
            ReportId::Feature(_) | ReportId::In(_) => {
                let mut report = Report::new(buf);
                report.field(BRIGHTNESS.try_get().unwrap());
                Some(report.len())
            }
            _ => None,
        }
    }

    fn set_report(&mut self, id: ReportId, data: &[u8]) -> OutResponse {
        match id {
            ReportId::Feature(_) if data.len() >= 2 => {
                let v = u16::from_le_bytes([data[0], data[1]]).min(MAX_BRIGHTNESS);
                BRIGHTNESS.sender().send(v);
                defmt::info!("brightness set to {}", v);
                OutResponse::Accepted
            }
            _ => OutResponse::Rejected,
        }
    }
}

struct PowerHandler;

impl RequestHandler for PowerHandler {
    fn get_report(&mut self, id: ReportId, buf: &mut [u8]) -> Option<usize> {
        match id {
            ReportId::Feature(_) | ReportId::In(_) => {
                let power = POWER_TELEMETRY.try_get().unwrap();
                let mut report = Report::new(buf);
                report
                    .field(power.voltage_mv)
                    .field(power.current_ma)
                    .field(power.power_mw);
                Some(report.len())
            }
            _ => None,
        }
    }
}

struct ThermalHandler;

impl RequestHandler for ThermalHandler {
    fn get_report(&mut self, id: ReportId, buf: &mut [u8]) -> Option<usize> {
        match id {
            ReportId::Feature(_) | ReportId::In(_) => {
                let thermal = THERMAL_TELEMETRY.try_get().unwrap();
                let mut report = Report::new(buf);
                report
                    .field(thermal.internal_decic)
                    .field(thermal.external1_decic);
                Some(report.len())
            }
            _ => None,
        }
    }
}

pub struct UsbPeripherals {
    pub usb: UsbDevice<'static, UsbDriver>,
    pub brightness_writer: HidWriter<'static, UsbDriver, 2>,
    pub power_writer: HidWriter<'static, UsbDriver, 8>,
    pub thermal_writer: HidWriter<'static, UsbDriver, 4>,
}

pub fn init(usb_driver: UsbDriver, unique_id: &'static str) -> UsbPeripherals {
    let mut builder = usb_builder(usb_driver, unique_id);

    static BRIGHTNESS_HANDLER: StaticCell<BrightnessHandler> = StaticCell::new();
    static BRIGHTNESS_STATE: StaticCell<HidState> = StaticCell::new();
    let brightness_writer = build_hid_writer(
        &mut builder,
        MONITOR_REPORT_DESCRIPTOR,
        BRIGHTNESS_HANDLER.init(BrightnessHandler),
        BRIGHTNESS_STATE.init(HidState::new()),
    );

    static POWER_HANDLER: StaticCell<PowerHandler> = StaticCell::new();
    static POWER_STATE: StaticCell<HidState> = StaticCell::new();
    let power_writer = build_hid_writer(
        &mut builder,
        POWER_REPORT_DESCRIPTOR,
        POWER_HANDLER.init(PowerHandler),
        POWER_STATE.init(HidState::new()),
    );

    static THERMAL_HANDLER: StaticCell<ThermalHandler> = StaticCell::new();
    static THERMAL_STATE: StaticCell<HidState> = StaticCell::new();
    let thermal_writer = build_hid_writer(
        &mut builder,
        THERMAL_REPORT_DESCRIPTOR,
        THERMAL_HANDLER.init(ThermalHandler),
        THERMAL_STATE.init(HidState::new()),
    );

    let usb = builder.build();

    UsbPeripherals {
        usb,
        brightness_writer,
        power_writer,
        thermal_writer,
    }
}

/// USB string descriptor `bLength` is 1 byte: max (255 - 2 header) / 2 = 126
/// UTF-16 code units.
const MAX_SERIAL_LEN: usize = 126;

fn usb_device_config(unique_id: &'static str) -> UsbConfig<'static> {
    let mut config = UsbConfig::new(0x1209, 0xCC02); // pid.codes shared testing VID:PID
    config.manufacturer = Some("CinemaControl");
    config.product = Some("CinemaControl Monitor Controller");
    config.serial_number = Some(clamp_to_string_descriptor(unique_id));
    config.max_power = 100;
    config.max_packet_size_0 = 64;
    config.device_class = 0x00;
    config.device_sub_class = 0x00;
    config.device_protocol = 0x00;
    config.composite_with_iads = false;
    config
}

fn clamp_to_string_descriptor(s: &str) -> &str {
    &s[..s.floor_char_boundary(MAX_SERIAL_LEN)]
}

fn usb_builder(usb_driver: UsbDriver, unique_id: &'static str) -> Builder<'static, UsbDriver> {
    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static MSOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 256]> = StaticCell::new();

    Builder::new(
        usb_driver,
        usb_device_config(unique_id),
        CONFIG_DESC.init([0; 256]),
        BOS_DESC.init([0; 256]),
        MSOS_DESC.init([0; 256]),
        CONTROL_BUF.init([0; 256]),
    )
}

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
        max_packet_size: N as u16,
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
    let mut brightness = BRIGHTNESS.receiver().unwrap();
    let mut value = brightness.try_get().unwrap();

    loop {
        if let Err(e) = writer.write(&value.to_le_bytes()).await {
            warn!("hid input report write failed: {:?}", e);
        }
        value = brightness.changed().await;
    }
}

#[embassy_executor::task]
pub async fn power_report_task(mut writer: HidWriter<'static, UsbDriver, 8>) -> ! {
    writer.ready().await;
    let mut power = POWER_TELEMETRY.receiver().unwrap();
    let mut value = power.try_get().unwrap();

    loop {
        let mut report_buf = [0u8; 8];
        Report::new(&mut report_buf)
            .field(value.voltage_mv)
            .field(value.current_ma)
            .field(value.power_mw);

        if let Err(e) = writer.write(&report_buf).await {
            warn!("power input report write failed: {:?}", e);
        }

        value = power.changed().await;
    }
}

#[embassy_executor::task]
pub async fn thermal_report_task(mut writer: HidWriter<'static, UsbDriver, 4>) -> ! {
    writer.ready().await;
    let mut thermal = THERMAL_TELEMETRY.receiver().unwrap();
    let mut value = thermal.try_get().unwrap();

    loop {
        let mut report_buf = [0u8; 4];
        Report::new(&mut report_buf)
            .field(value.internal_decic)
            .field(value.external1_decic);

        if let Err(e) = writer.write(&report_buf).await {
            warn!("thermal input report write failed: {:?}", e);
        }

        value = thermal.changed().await;
    }
}
