//! Small helpers for building HID report byte buffers.

/// A value that can serialize itself as little-endian bytes for a HID report
/// field, so report fields can be written as `.field(value)` instead of
/// spelling out `.to_le_bytes()` at every call site.
pub trait ToLeBytes<const N: usize> {
    fn to_le_bytes(self) -> [u8; N];
}

impl ToLeBytes<2> for u16 {
    fn to_le_bytes(self) -> [u8; 2] {
        u16::to_le_bytes(self)
    }
}

impl ToLeBytes<2> for i16 {
    fn to_le_bytes(self) -> [u8; 2] {
        i16::to_le_bytes(self)
    }
}

impl ToLeBytes<4> for u32 {
    fn to_le_bytes(self) -> [u8; 4] {
        u32::to_le_bytes(self)
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

    pub fn field<T: ToLeBytes<N>, const N: usize>(&mut self, value: T) -> &mut Self {
        let bytes = value.to_le_bytes();
        self.buf[self.len..self.len + N].copy_from_slice(&bytes);
        self.len += N;
        self
    }

    pub fn len(&self) -> usize {
        self.len
    }
}
