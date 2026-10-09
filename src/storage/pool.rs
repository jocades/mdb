use std::{collections::HashMap, io};

use super::{Disk, Header, PAGE_SIZE, PageBuf, PageId, PageView, page_kind};
use crate::{
    bytes::{Buf, BufMut},
    storage::MemDisk,
};

pub const DEFAULT_POOL_CAPACITY: usize = 100;

#[derive(Debug)]
pub struct Pool<D: Disk> {
    disk: D,
    header: Header,
    frames: Box<[Frame]>,
    by_page: HashMap<PageId, FrameId>,
    hand: FrameId,
}

type FrameId = usize;

struct Frame {
    buf: PageBuf,
    pageid: Option<PageId>,
    dirty: bool,
    referenced: bool,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            buf: [0; PAGE_SIZE],
            pageid: None,
            dirty: false,
            referenced: false,
        }
    }
}

impl<D: Disk> Pool<D> {
    pub fn with_capacity(mut disk: D, cap: usize) -> io::Result<Self> {
        assert!(cap != 0);

        let header = if disk.is_empty() {
            let page0 = disk.alloc()?;
            let header = Header::default();
            disk.write(page0, &header.encode())?;
            header
        } else {
            let mut buf = [0u8; PAGE_SIZE];
            disk.read(0, &mut buf)?;
            Header::decode(&buf)?
        };

        Ok(Self {
            disk,
            header,
            frames: (0..cap).map(|_| Frame::default()).collect(),
            by_page: HashMap::with_capacity(cap),
            hand: 0,
        })
    }

    fn evict(&mut self) -> io::Result<FrameId> {
        loop {
            let i = self.hand;
            self.hand = (self.hand + 1) % self.frames.len();

            let frame = &mut self.frames[i];
            let Some(old) = frame.pageid else { return Ok(i) };

            if frame.referenced {
                frame.referenced = false;
                continue;
            }

            if frame.dirty {
                self.disk.write(old, &frame.buf)?;
                frame.dirty = false;
            }

            frame.pageid = None;
            self.by_page.remove(&old);

            return Ok(i);
        }
    }

    fn install(&mut self, i: FrameId, id: PageId, dirty: bool) {
        let frame = &mut self.frames[i];
        frame.pageid = Some(id);
        frame.dirty = dirty;
        frame.referenced = true;
        self.by_page.insert(id, i);
    }

    fn fetch(&mut self, id: PageId) -> io::Result<FrameId> {
        if let Some(&i) = self.by_page.get(&id) {
            self.frames[i].referenced = true;
            return Ok(i);
        }

        let i = self.evict()?;
        self.disk.read(id, &mut self.frames[i].buf)?;
        self.install(i, id, false);
        Ok(i)
    }

    pub fn reader(&mut self, id: PageId) -> io::Result<&PageBuf> {
        debug_assert!(id < self.disk.len());
        let i = self.fetch(id)?;
        Ok(&self.frames[i].buf)
    }

    pub fn writer(&mut self, id: PageId) -> io::Result<&mut PageBuf> {
        debug_assert!(id < self.disk.len());
        let i = self.fetch(id)?;
        self.frames[i].dirty = true;
        Ok(&mut self.frames[i].buf)
    }

    pub fn get<P: PageView + ?Sized>(&mut self, id: PageId) -> io::Result<&P> {
        let buf = self.reader(id)?;
        Ok(P::open(buf)?)
    }

    pub fn get_mut<P: PageView + ?Sized>(&mut self, id: PageId) -> io::Result<&mut P> {
        let buf = self.writer(id)?;
        Ok(P::open_mut(buf)?)
    }

    pub fn alloc(&mut self) -> io::Result<PageId> {
        if self.header.free_head != 0 {
            let id = self.header.free_head;
            let i = self.fetch(id)?;
            let frame = &mut self.frames[i];
            assert!(frame.buf[0] == page_kind::FREE, "corrupt freelist");
            self.header.free_head = frame.buf.get_u32(4);
            frame.buf.fill(0);
            frame.dirty = true;
            Ok(id)
        } else {
            let i = self.evict()?;
            let id = self.disk.alloc()?;
            self.frames[i].buf.fill(0);
            self.install(i, id, true);
            Ok(id)
        }
    }

    pub fn free(&mut self, id: PageId) -> io::Result<()> {
        assert!(id != 0 && id < self.disk.len());
        let i = self.fetch(id)?;
        let frame = &mut self.frames[i];
        assert!(frame.buf[0] != page_kind::FREE, "double free");
        frame.buf.fill(0);
        frame.buf.put_u8(0, page_kind::FREE);
        frame.buf.put_u32(4, self.header.free_head);
        frame.dirty = true;
        self.header.free_head = id;
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        for frame in &mut self.frames {
            if let (Some(id), true) = (frame.pageid, frame.dirty) {
                self.disk.write(id, &frame.buf)?;
                frame.dirty = false;
            }
        }
        self.disk.write(0, &self.header.encode())?;
        self.disk.sync()
    }

    pub fn len(&self) -> u32 {
        self.disk.len()
    }
}

impl<D: Disk> Drop for Pool<D> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

impl Pool<MemDisk> {
    pub fn reads(&self) -> usize {
        self.disk.reads
    }

    pub fn writes(&self) -> usize {
        self.disk.writes
    }
}

// Omit printing the buffer when debugging
impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("pageid", &self.pageid)
            .field("dirty", &self.dirty)
            .field("referenced", &self.referenced)
            .finish()
    }
}
