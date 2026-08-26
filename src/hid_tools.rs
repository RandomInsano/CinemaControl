//! Small helpers for building HID report byte buffers.

use core::sync::atomic::{AtomicI16, AtomicU16, Ordering};

/// An atomic that can read itself out as little-endian bytes, so report
/// fields can be written as `.load_le_bytes()` instead of spelling out
/// `.load(Ordering::Relaxed).to_le_bytes()` at every call site.
pub trait LoadLeBytes<const N: usize> {
    fn load_le_bytes(&self) -> [u8; N];
}

impl LoadLeBytes<2> for AtomicU16 {
    fn load_le_bytes(&self) -> [u8; 2] {
        self.load(Ordering::Relaxed).to_le_bytes()
    }
}

impl LoadLeBytes<2> for AtomicI16 {
    fn load_le_bytes(&self) -> [u8; 2] {
        self.load(Ordering::Relaxed).to_le_bytes()
    }
}

/// Fills a HID report buffer one field at a time. Each [`Report::field`]
/// call appends its bytes and returns `self` for chaining, so a report is
/// just the list of fields it contains — no caller ever computes a slice
/// range or running offset by hand.
pub struct Report<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> Report<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    pub fn field<T: LoadLeBytes<N>, const N: usize>(&mut self, value: &T) -> &mut Self {
        let bytes = value.load_le_bytes();
        self.buf[self.len..self.len + N].copy_from_slice(&bytes);
        self.len += N;
        self
    }

    pub fn len(&self) -> usize {
        self.len
    }
}
