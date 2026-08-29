use crate::regs::Register;

/// One of the up-to-four temperature channels the family exposes.
/// `External3` only exists on the EMC1404 — [`crate::Emc1403::identify`]
/// reports which part is actually on the bus, but nothing here stops you
/// from addressing `External3` on an EMC1403; the chip simply won't have
/// wired it to anything, and mirrors whatever driver-side bookkeeping the
/// unmapped registers happen to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Internal,
    External1,
    External2,
    External3,
}

impl Channel {
    /// (high byte, low byte) — see the Data Read Interlock note on
    /// [`crate::Emc1403::read_temp_c`] for why the order of reading these
    /// matters.
    pub(crate) fn temp_regs(self) -> (Register, Register) {
        match self {
            Channel::Internal => (Register::InternalTempHigh, Register::InternalTempLow),
            Channel::External1 => (Register::External1TempHigh, Register::External1TempLow),
            Channel::External2 => (Register::External2TempHigh, Register::External2TempLow),
            Channel::External3 => (Register::External3TempHigh, Register::External3TempLow),
        }
    }

    /// Whole-degree high-limit register. The fractional low-byte limit
    /// registers (0x13/0x14/0x17/0x18/0x2E/0x2F) exist but aren't modeled
    /// here — nothing in this driver's target use needs sub-degree limits,
    /// and the raw registers are still reachable via
    /// [`crate::Emc1403::read_register`]/[`crate::Emc1403::write_register`]
    /// if that changes.
    pub(crate) fn high_limit_reg(self) -> Register {
        match self {
            Channel::Internal => Register::InternalHighLimit,
            Channel::External1 => Register::External1HighLimitHigh,
            Channel::External2 => Register::External2HighLimitHigh,
            Channel::External3 => Register::External3HighLimitHigh,
        }
    }

    pub(crate) fn low_limit_reg(self) -> Register {
        match self {
            Channel::Internal => Register::InternalLowLimit,
            Channel::External1 => Register::External1LowLimitHigh,
            Channel::External2 => Register::External2LowLimitHigh,
            Channel::External3 => Register::External3LowLimitHigh,
        }
    }

    pub(crate) fn therm_limit_reg(self) -> Register {
        match self {
            Channel::Internal => Register::InternalThermLimit,
            Channel::External1 => Register::External1ThermLimit,
            Channel::External2 => Register::External2ThermLimit,
            Channel::External3 => Register::External3ThermLimit,
        }
    }

    /// Bit position shared by the High/Low/THERM Limit Status registers
    /// (0x35/0x36/0x37) and the Channel Mask register (0x1F) for this
    /// channel — see [`crate::flags`].
    pub(crate) fn status_bit(self) -> u8 {
        match self {
            Channel::Internal => 0,
            Channel::External1 => 1,
            Channel::External2 => 2,
            Channel::External3 => 3,
        }
    }
}
