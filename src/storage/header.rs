use super::{Corrupt, PAGE_SIZE, PageBuf, PageId};
use crate::bytes::{Buf, BufMut};

/**
The database file header aka `Page 0`

Since it's touched only at open and flush, it's the only page which gets copied and encoded
into a struct, the other page views, like `HeapPage`, can only be used as `&T` or `&mut T`,
this is intentional to avoid unecessary byte copies, instead they read/write directly from/to
the buffer owned by the `Pool`.

# Layout

All integers are stored as big-endian unsigned 32-bit

| offset | field                                                               |
|------------------------------------------------------------------------------|
|  0     | magic - "mdb\0"                                                     |
|  4     | version - refuse files from an incompatible layout                  |
|  8     | page size - a file with a diffrent page size is unreadable, so fail |
| 12     | free head - root of the free list                                   |
*/
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub free_head: PageId, // 0 = free empty list
}

const MAGIC: &[u8; 4] = b"mdb\0";
const VERSION: u32 = 1;

// Offsets
const VERS: usize = 4;
const SIZE: usize = 8;
const FREE_HEAD: usize = 12;

impl Header {
    pub fn encode(&self) -> PageBuf {
        let mut buf = [0u8; PAGE_SIZE];
        buf.put_slice(0, MAGIC);
        buf.put_u32(VERS, VERSION);
        buf.put_u32(SIZE, PAGE_SIZE as u32);
        buf.put_u32(FREE_HEAD, self.free_head);
        buf
    }

    pub fn decode(buf: &PageBuf) -> Result<Self, Corrupt> {
        // todo: concrete corrupt errors
        if &buf[..4] != MAGIC
            || buf.get_u32(VERS) != VERSION
            || buf.get_u32(SIZE) != PAGE_SIZE as u32
        {
            bail!(Corrupt)
        }

        Ok(Self {
            free_head: buf.get_u32(FREE_HEAD),
        })
    }
}
