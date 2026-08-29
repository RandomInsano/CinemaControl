//! A cheap, `Copy` `embedded-hal-async` [`I2c`] handle onto a bus shared by
//! more than one independent task. `board::Board::smbus` hands out a
//! `&'static` mutex rather than the bus itself, specifically so `smbus.rs`'s
//! two telemetry tasks (INA219, EMC1403) can each hold their own driver
//! instance for their whole lifetime instead of reconstructing one every
//! poll cycle just to get a short-lived exclusive borrow of a single shared
//! `SmbusBus` value.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_hal_async::i2c::{ErrorType, I2c, Operation};

use crate::board::SmbusBus;

#[derive(Clone, Copy)]
pub struct SharedI2c(&'static Mutex<CriticalSectionRawMutex, SmbusBus>);

impl SharedI2c {
    pub fn new(bus: &'static Mutex<CriticalSectionRawMutex, SmbusBus>) -> Self {
        Self(bus)
    }
}

impl ErrorType for SharedI2c {
    type Error = <SmbusBus as ErrorType>::Error;
}

impl I2c for SharedI2c {
    /// Locks the underlying bus for exactly this transaction's duration —
    /// `I2c::read`/`write`/`write_read` all forward to this via the
    /// trait's own default implementations, so it's the only method this
    /// needs to implement.
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.0.lock().await.transaction(address, operations).await
    }
}
