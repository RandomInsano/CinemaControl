#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error<E> {
    Bus(E),
    /// Returned by [`crate::Ina219::current_a`]/[`crate::Ina219::power_w`]
    /// when [`crate::Ina219::calibrate`] hasn't been called yet (or not
    /// since the last [`crate::Ina219::reset`]). Guards against the #1
    /// INA219 driver bug: an uncalibrated device silently and permanently
    /// reads Current/Power back as 0x0000, which is indistinguishable from
    /// a real 0A/0W reading unless this is caught before it reaches a
    /// caller.
    NotCalibrated,
}

impl<E> From<E> for Error<E> {
    fn from(e: E) -> Self {
        Error::Bus(e)
    }
}
