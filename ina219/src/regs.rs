//! Register addresses, INA219 datasheet (TI SBOS448) register map. Unlike
//! the EMC1403's ~40-entry map, this part only has six register-pointer
//! values, 0x00-0x05 — there is no auto-increment across registers, so one
//! pointer write/select stays in effect for however many reads follow until
//! it's changed again.

/// A register address. [`crate::Ina219::read_register`]/
/// [`crate::Ina219::write_register`] take `impl Into<u8>`, so a plain `u8`
/// still works for an out-of-range pointer value too, though the datasheet
/// doesn't document what a real device does with one (see the module doc
/// comment on [`crate`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Register {
    Configuration = 0x00,
    ShuntVoltage = 0x01,
    BusVoltage = 0x02,
    Power = 0x03,
    Current = 0x04,
    Calibration = 0x05,
}

impl From<Register> for u8 {
    fn from(reg: Register) -> u8 {
        reg as u8
    }
}
