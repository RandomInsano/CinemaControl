//! USB HID: VESA/USB Monitor Control Class brightness control, plus a
//! Power Device interface bundling PSU telemetry (voltage/current still
//! placeholder, temperature real via the EMC1403).

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
use crate::smbus::PSU_TELEMETRY;

/// Current backlight brightness, 0..=1023: both the value (readable
/// synchronously via `try_get`) and the change notification (awaitable via a
/// receiver's `changed`) for the HID Feature/Input report and the PWM duty
/// cycle. Three receivers: the PWM task, the storage task, and the HID Input
/// report task.
pub static BRIGHTNESS: Watch<CriticalSectionRawMutex, u16, 3> = Watch::new_with(512);

pub const MAX_BRIGHTNESS: u16 = 1023;

/// Sets the startup brightness restored from flash, same as any other
/// [`BRIGHTNESS`] update: every receiver's first `changed` fires with this
/// value. `pwm.rs` and [`hid_report_task`] just reapply it, harmlessly.
/// `storage.rs` is the one consumer where re-saving it immediately would
/// matter, but its `save` already no-ops when the value matches what's on
/// flash — which this always does, since it's the value `storage.rs` itself
/// just loaded — so no special-casing is needed there either.
pub fn restore_brightness(value: u16) {
    BRIGHTNESS.sender().send(value.min(MAX_BRIGHTNESS));
}

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
/// it's the only report this interface has, so Voltage + Current + two
/// Temperature channels just concatenate into one 8-byte Input and one
/// 8-byte Feature report. Voltage/Current are unsigned millivolts/
/// milliamps (still placeholder — see `smbus::PsuTelemetry`); the two
/// Temperature fields are signed tenths of a degree C (-40.0 to 150.0) and
/// are real EMC1403 reads. Both Temperature fields share the same usage
/// (0x36): HID assigns repeated local usage tags to a multi-count item in
/// declaration order, so the two tags below map to Internal Diode and
/// External Diode 1 respectively, in the same order
/// `smbus::PsuTelemetry`/[`psu_report_task`] write them in.
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
    0x95, 0x02,       //   Report Count (2)
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

/// Read-only: reports whatever's in the PSU telemetry statics, rejects
/// writes. Consistent with `src/smbus.rs` staying read-only until we
/// actually know the PA-2311-02A's own PMBus register map (the temperature
/// fields, unlike voltage/current, are already real — see
/// `smbus::PsuTelemetry`).
struct PsuHandler;

impl RequestHandler for PsuHandler {
    fn get_report(&mut self, id: ReportId, buf: &mut [u8]) -> Option<usize> {
        match id {
            ReportId::Feature(_) | ReportId::In(_) => {
                let telemetry = PSU_TELEMETRY.try_get().unwrap();
                let mut report = Report::new(buf);
                report
                    .field(telemetry.voltage_mv)
                    .field(telemetry.current_ma)
                    .field(telemetry.internal_decic)
                    .field(telemetry.external1_decic);
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
    pub psu_writer: HidWriter<'static, UsbDriver, 8>,
}

/// Sets up the USB device with two HID interfaces: the VESA Monitor
/// brightness control, and the Power Device telemetry interface.
/// `unique_id` (currently the RP2040 board's factory flash ID, see
/// `board.rs`) becomes the USB serial number, so every board is
/// distinguishable out of the box — no provisioning step, and nothing for
/// `cinectl` to set. Taken as a plain `&str` (clamped to what a USB string
/// descriptor can hold in [`usb_device_config`]) rather than something
/// shaped around how it's derived today, so a different source later —
/// another chip's ID scheme, or a user-assigned name — is just a different
/// caller, not a change here. Ready to spawn via [`usb_task`],
/// [`hid_report_task`] and [`psu_report_task`].
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

/// A USB string descriptor's `bLength` is one byte, and the descriptor is 2
/// header bytes + 2 bytes per UTF-16 code unit — so at most
/// `(255 - 2) / 2 = 126` UTF-16 code units fit. `embassy-usb` doesn't check
/// this (an oversize string just silently truncates `bLength`, corrupting
/// the descriptor), so `usb_device_config` clamps to it itself. Clamped by
/// *byte* length, not actual UTF-16 length — conservative, since UTF-8 never
/// takes fewer bytes than UTF-16 takes code units, so this can only truncate
/// shorter than strictly necessary for non-ASCII input, never longer.
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

/// Truncates `s` to at most [`MAX_SERIAL_LEN`] bytes, at a `char` boundary.
fn clamp_to_string_descriptor(s: &str) -> &str {
    &s[..s.floor_char_boundary(MAX_SERIAL_LEN)]
}

/// Allocates the descriptor/control buffers `Builder` needs (as `'static`
/// storage, since the `UsbDevice` it produces gets spawned as a task) and
/// starts a `Builder` from them.
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
pub async fn psu_report_task(mut writer: HidWriter<'static, UsbDriver, 8>) -> ! {
    writer.ready().await;
    let mut telemetry = PSU_TELEMETRY.receiver().unwrap();
    let mut value = telemetry.try_get().unwrap();

    loop {
        let mut report_buf = [0u8; 8];
        Report::new(&mut report_buf)
            .field(value.voltage_mv)
            .field(value.current_ma)
            .field(value.internal_decic)
            .field(value.external1_decic);

        if let Err(e) = writer.write(&report_buf).await {
            warn!("psu input report write failed: {:?}", e);
        }

        value = telemetry.changed().await;
    }
}
