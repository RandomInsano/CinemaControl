//! HID report wire format, mirroring `firmware/src/hid.rs`'s report
//! descriptors. Feature reports carry a leading Report ID byte (always 0);
//! Input reports read via `HidDevice::read` don't.

pub fn brightness_from_bytes(bytes: [u8; 2]) -> u16 {
    u16::from_le_bytes(bytes)
}

pub fn brightness_feature_report(value: u16) -> [u8; protocol::BRIGHTNESS_REPORT_LEN + 1] {
    let [lo, hi] = value.min(protocol::MAX_BRIGHTNESS).to_le_bytes();
    [0, lo, hi]
}
