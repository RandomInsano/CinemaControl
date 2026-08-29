//! Register addresses, Table 6.1 of DS20005272A. Only the primary address
//! is listed for mirrored registers (0x03/0x09, 0x04/0x0A, 0x05/0x0B,
//! 0x06/0x0C, 0x07/0x0D, 0x08/0x0E) — either address reaches the same
//! underlying value, so the driver only ever needs one.

/// A register address. [`crate::Emc1403::read_register`]/
/// [`crate::Emc1403::write_register`] take `impl Into<u8>`, so a plain
/// `u8` still works for anything not listed here; this exists so the
/// common case can't typo an address, and so registers this driver doesn't
/// give a typed accessor to (Beta Configuration, Ideality Factor, Filter
/// Control, Scratchpad) are still nameable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Register {
    InternalTempHigh = 0x00,
    External1TempHigh = 0x01,
    Status = 0x02,
    Configuration = 0x03,
    ConversionRate = 0x04,
    InternalHighLimit = 0x05,
    InternalLowLimit = 0x06,
    External1HighLimitHigh = 0x07,
    External1LowLimitHigh = 0x08,
    OneShot = 0x0F,
    External1TempLow = 0x10,
    Scratchpad1 = 0x11,
    Scratchpad2 = 0x12,
    External1HighLimitLow = 0x13,
    External1LowLimitLow = 0x14,
    External2HighLimitHigh = 0x15,
    External2LowLimitHigh = 0x16,
    External2HighLimitLow = 0x17,
    External2LowLimitLow = 0x18,
    External1ThermLimit = 0x19,
    External2ThermLimit = 0x1A,
    ExternalDiodeFault = 0x1B,
    ChannelMask = 0x1F,
    InternalThermLimit = 0x20,
    ThermHysteresis = 0x21,
    ConsecutiveAlert = 0x22,
    External2TempHigh = 0x23,
    External2TempLow = 0x24,
    External1BetaConfig = 0x25,
    External2BetaConfig = 0x26,
    External1Ideality = 0x27,
    External2Ideality = 0x28,
    InternalTempLow = 0x29,
    External3TempHigh = 0x2A,
    External3TempLow = 0x2B,
    External3HighLimitHigh = 0x2C,
    External3LowLimitHigh = 0x2D,
    External3HighLimitLow = 0x2E,
    External3LowLimitLow = 0x2F,
    External3ThermLimit = 0x30,
    External3Ideality = 0x31,
    HighLimitStatus = 0x35,
    LowLimitStatus = 0x36,
    ThermLimitStatus = 0x37,
    FilterControl = 0x40,
    ProductId = 0xFD,
    ManufacturerId = 0xFE,
    Revision = 0xFF,
}

impl From<Register> for u8 {
    fn from(reg: Register) -> u8 {
        reg as u8
    }
}

// Values, not addresses — read back from `Register::ManufacturerId` /
// `Register::ProductId`, not registers themselves.
pub const MANUFACTURER_ID_VALUE: u8 = 0x5D;
pub const PRODUCT_ID_EMC1403: u8 = 0x21;
pub const PRODUCT_ID_EMC1404: u8 = 0x25;
