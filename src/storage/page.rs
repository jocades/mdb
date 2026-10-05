pub const PAGE_SIZE: usize = 4096;

pub type PageId = u32;
pub type PageBuf = [u8; PAGE_SIZE];

#[derive(Debug)]
pub struct Corrupt;

/// A view over page bytes which knows how to validate itself
pub trait PageView {
    fn open(buf: &[u8]) -> Result<&Self, Corrupt>;
    fn open_mut(buf: &mut [u8]) -> Result<&mut Self, Corrupt>;
}

impl From<Corrupt> for std::io::Error {
    fn from(_: Corrupt) -> Self {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "corrupt")
    }
}

pub mod page_kind {
    pub const UNINIT: u8 = 0; // zeroed page, never formatted
    pub const FREE: u8 = 1; // on the free list
    pub const HEAP: u8 = 2; // slotted page holding table rows
    // pub const OVERFLOW: u8 = 3;
}
