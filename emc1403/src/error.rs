#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error<E> {
    Bus(E),
    /// [`crate::Emc1403::identify`]/[`crate::Emc1403::probe`] read something
    /// other than a Microchip EMC1403/EMC1404 back. Surfaced as a distinct
    /// variant (not folded into a generic bus error) because this bus can
    /// also carry other devices at other addresses — a wrong-address bug
    /// should fail loudly rather than return plausible-looking garbage.
    UnexpectedDevice { product: u8, manufacturer: u8 },
}

impl<E> From<E> for Error<E> {
    fn from(e: E) -> Self {
        Error::Bus(e)
    }
}
