//! A cheap, `Copy` `embedded-hal-async` [`I2c`] handle onto a bus shared by
//! more than one independent task.

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
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.0.lock().await.transaction(address, operations).await
    }
}
