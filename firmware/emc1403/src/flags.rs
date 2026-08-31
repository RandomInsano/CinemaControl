//! Bit layouts shared across several registers. Grouped here rather than
//! next to each accessor because the High/Low/THERM Limit Status registers
//! (0x35/0x36/0x37) and Channel Mask (0x1F) all share the same four-channel
//! bit shape — see datasheet S5.2-5.4, S12.

use crate::Channel;

bitflags::bitflags! {
    /// Status register (0x02). Note that none of these bits clear by
    /// reading 0x02 itself — HIGH/LOW/FAULT clear via their own
    /// read-to-clear registers, and THERM only self-clears once temperature
    /// drops below (THERM limit - hysteresis). See
    /// [`crate::Emc1403::status`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Status: u8 {
        const BUSY  = 1 << 7;
        const HIGH  = 1 << 4;
        const LOW   = 1 << 3;
        const FAULT = 1 << 2;
        const THERM = 1 << 1;
    }

    /// Configuration register (0x03/0x09, mirrored).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Configuration: u8 {
        const MASK_ALL   = 1 << 7;
        /// Set = Standby (not converting). Clear = Active.
        const STANDBY    = 1 << 6;
        /// Set = ALERT pin in comparator mode. Clear = interrupt mode.
        const ALERT_COMP = 1 << 5;
        /// Set = REC disabled for External Diode 1.
        const RECD1      = 1 << 4;
        /// Set = REC disabled for External Diode 2 and 3.
        const RECD2      = 1 << 3;
        /// Set = extended range (-64..+191.875C, offset binary). Clear =
        /// standard range (0..+127.875C, unsigned binary).
        const RANGE      = 1 << 2;
        /// Set = dynamic averaging disabled (max 1x / 11-bit).
        const DAVG_DIS   = 1 << 1;
        /// EMC1404 only. Clear = anti-parallel diode mode on DP2/DN2.
        const APDD       = 1 << 0;
    }

    /// Shared bit shape for High Limit Status (0x35), Low Limit Status
    /// (0x36), and THERM Limit Status (0x37) — but *not* their clear
    /// semantics, which differ per register (see the doc comments on
    /// [`crate::Emc1403::take_high_limit_status`],
    /// [`crate::Emc1403::take_low_limit_status`], and
    /// [`crate::Emc1403::peek_therm_status`]).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct LimitStatus: u8 {
        const EXTERNAL3 = 1 << 3;
        const EXTERNAL2 = 1 << 2;
        const EXTERNAL1 = 1 << 1;
        const INTERNAL  = 1 << 0;
    }

    /// External Diode Fault register (0x1B). No bit exists for the internal
    /// diode — there's no fault concept for an on-die sensor.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DiodeFault: u8 {
        const EXTERNAL3 = 1 << 3;
        const EXTERNAL2 = 1 << 2;
        const EXTERNAL1 = 1 << 1;
    }

    /// Channel Mask register (0x1F). A set bit excludes that channel from
    /// asserting ALERT (on limit violation or fault) — THERM is never
    /// affected by this register, regardless.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ChannelMask: u8 {
        const EXTERNAL3 = 1 << 3;
        const EXTERNAL2 = 1 << 2;
        const EXTERNAL1 = 1 << 1;
        const INTERNAL  = 1 << 0;
    }
}

impl LimitStatus {
    /// The single bit corresponding to `ch`, for `.intersects(..)` checks.
    pub fn for_channel(ch: Channel) -> Self {
        Self::from_bits_truncate(1 << ch.status_bit())
    }
}

impl DiodeFault {
    /// The single bit corresponding to `ch` — always empty for
    /// `Channel::Internal`, which has no fault bit.
    pub fn for_channel(ch: Channel) -> Self {
        Self::from_bits_truncate(1 << ch.status_bit())
    }
}

impl ChannelMask {
    /// The single bit corresponding to `ch`.
    pub fn for_channel(ch: Channel) -> Self {
        Self::from_bits_truncate(1 << ch.status_bit())
    }
}
