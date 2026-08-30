//! Persists brightness across power cycles in the last two flash erase
//! sectors, debounced so dragging a brightness slider doesn't wear out the
//! flash.

use core::ops::Range;

use defmt::warn;
use sequential_storage::cache::{Cache, Uncached};
use sequential_storage::map::{MapConfig, MapStorage};

use crate::board::{BoardFlash, FLASH_SIZE};
use crate::hid;

const FLASH_RANGE: Range<u32> = (FLASH_SIZE as u32 - 8 * 1024)..FLASH_SIZE as u32;

const DEBOUNCE: embassy_time::Duration = embassy_time::Duration::from_secs(30);

type Store = MapStorage<(), BoardFlash, Cache<Uncached, Uncached, Uncached, ()>>;

pub fn init(flash: BoardFlash) -> Store {
    MapStorage::new(
        flash,
        const { MapConfig::new(FLASH_RANGE) },
        Cache::new_uncached(),
    )
}

pub async fn load(store: &mut Store) -> Option<u16> {
    let mut buf = [0u8; 32];
    match store.fetch_item::<u16>(&mut buf, &()).await {
        Ok(value) => value,
        Err(e) => {
            warn!("brightness flash read failed: {:?}", e);
            None
        }
    }
}

#[embassy_executor::task]
pub async fn task(mut store: Store) -> ! {
    let mut brightness = hid::BRIGHTNESS.receiver().unwrap();
    loop {
        let saved = brightness.changed().await;
        save(&mut store, saved).await;

        embassy_time::Timer::after(DEBOUNCE).await;
        if let Some(v) = brightness.try_changed()
            && v != saved
        {
            save(&mut store, v).await;
        }
    }
}

async fn save(store: &mut Store, value: u16) {
    if load(store).await == Some(value) {
        return;
    }

    let mut buf = [0u8; 32];
    if let Err(e) = store.store_item(&mut buf, &(), &value).await {
        warn!("brightness flash write failed: {:?}", e);
    }
}
