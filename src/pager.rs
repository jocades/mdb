#![allow(unused)]
use std::{collections::HashMap, fs::File, io, os::unix::fs::FileExt, path::Path};

use crate::bytes::{Buf, BufMut};

const PAGE_SIZE: usize = 4096;

pub type PageId = u32;
pub type PageBuf = [u8; PAGE_SIZE];

pub type FrameId = usize;

const MAGIC: &[u8; 8] = b"RUSTYDB\0";

#[derive(Debug)]
struct Pager<D: Disk> {
    disk: D,
    free_head: PageId, // 0 = empty; page 0 is the header never a data page
    frames: Box<[Frame]>,
    by_page: HashMap<PageId, FrameId>,
    hand: FrameId,
}

struct Frame {
    buf: PageBuf,
    pageid: Option<PageId>,
    dirty: bool,
    referenced: bool,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            buf: [0u8; PAGE_SIZE],
            pageid: None,
            dirty: false,
            referenced: false,
        }
    }
}

fn corrupt(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[inline]
fn offset_of(id: PageId) -> u64 {
    id as u64 * PAGE_SIZE as u64
}

// Header (page 0):
// ```txt
// 0..8    ---  magic
// 8..12   u32  page_size
// 12..16  u32  free_head
// ```

fn write_header<D: Disk>(disk: &mut D, free_head: PageId) -> io::Result<()> {
    let mut buf = [0u8; PAGE_SIZE];
    buf[0..8].copy_from_slice(MAGIC);
    buf.put_u32(8, PAGE_SIZE as u32);
    buf.put_u32(12, free_head);
    disk.write(0, &buf)
}

#[rustfmt::skip]
fn read_header<D: Disk>(disk: &mut D) -> io::Result<PageId> {
    let mut buf = [0u8; PAGE_SIZE];
    disk.read(0, &mut buf)?;

    ensure!(&buf[0..8] == MAGIC, corrupt("bad magic"));
    ensure!(buf.get_u32(8) as usize == PAGE_SIZE, corrupt("page size mismatch"));

    let free_head = buf.get_u32(12);
    Ok(free_head)
}

impl<D: Disk> Pager<D> {
    pub fn open(mut disk: D, cap: usize) -> io::Result<Self> {
        assert!(cap != 0);

        let free_head = if disk.len() == 0 {
            disk.alloc()?; // page 0
            write_header(&mut disk, 0)?;
            0
        } else {
            read_header(&mut disk)?
        };

        Ok(Self {
            disk,
            free_head,
            frames: (0..cap).map(|_| Frame::default()).collect(),
            by_page: HashMap::new(),
            hand: 0,
        })
    }

    fn evict(&mut self) -> io::Result<FrameId> {
        loop {
            let i = self.hand;
            self.hand = (self.hand + 1) % self.frames.len();

            let f = &mut self.frames[i];
            let Some(old) = f.pageid else { return Ok(i) };

            if f.referenced {
                f.referenced = false;
                continue;
            }

            if f.dirty {
                self.disk.write(old, &f.buf)?;
                f.dirty = false;
            }

            f.pageid = None;
            self.by_page.remove(&old);

            return Ok(i);
        }
    }

    fn install(&mut self, i: FrameId, id: PageId, dirty: bool) {
        let f = &mut self.frames[i];
        f.pageid = Some(id);
        f.dirty = dirty;
        f.referenced = true;
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

    pub fn with_page<R>(&mut self, id: PageId, f: impl FnOnce(&PageBuf) -> R) -> io::Result<R> {
        debug_assert!(id < self.disk.len());
        let i = self.fetch(id)?;
        Ok(f(&self.frames[i].buf))
    }

    pub fn with_page_mut<R>(
        &mut self,
        id: PageId,
        f: impl FnOnce(&mut PageBuf) -> R,
    ) -> io::Result<R> {
        debug_assert!(id < self.disk.len());
        let i = self.fetch(id)?;
        self.frames[i].dirty = true;
        Ok(f(&mut self.frames[i].buf))
    }

    pub fn alloc(&mut self) -> io::Result<PageId> {
        if self.free_head != 0 {
            let id = self.free_head;
            let i = self.fetch(id)?;
            let f = &mut self.frames[i];
            self.free_head = f.buf.get_u32(0);
            f.buf.fill(0);
            f.dirty = true;
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
        debug_assert!(id < self.disk.len());
        let i = self.fetch(id)?;
        let f = &mut self.frames[i];
        f.buf.fill(0);
        f.buf.put_u32(0, self.free_head);
        f.dirty = true;
        self.free_head = id;
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        for f in &mut self.frames {
            if let (Some(id), true) = (f.pageid, f.dirty) {
                self.disk.write(id, &f.buf)?;
                f.dirty = false;
            }
        }
        write_header(&mut self.disk, self.free_head)?;
        self.disk.sync()
    }
}

impl<D: Disk> Drop for Pager<D> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("pageid", &self.pageid)
            .field("dirty", &self.dirty)
            .field("referenced", &self.referenced)
            .finish()
    }
}

/// Page I/O
pub trait Disk {
    fn read(&mut self, id: PageId, buf: &mut PageBuf) -> io::Result<()>;
    fn write(&mut self, id: PageId, buf: &PageBuf) -> io::Result<()>;
    fn alloc(&mut self) -> io::Result<PageId>;
    fn len(&self) -> u32;
    fn sync(&mut self) -> io::Result<()>;
}

pub struct FileDisk {
    file: File,
    pages: u32,
}

impl FileDisk {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::options()
            .read(true)
            .create(true)
            .write(true)
            .open(path)?;

        let len = file.metadata()?.len();

        ensure!(
            len % PAGE_SIZE as u64 == 0,
            corrupt("file size is not a multiple of page size")
        );

        Ok(Self {
            file,
            pages: (len / PAGE_SIZE as u64) as u32,
        })
    }
}

impl Disk for FileDisk {
    fn read(&mut self, id: PageId, buf: &mut PageBuf) -> io::Result<()> {
        assert!(id < self.pages);
        self.file.read_exact_at(buf, offset_of(id))
    }

    fn write(&mut self, id: PageId, buf: &PageBuf) -> io::Result<()> {
        assert!(id < self.pages);
        self.file.write_all_at(buf, offset_of(id))
    }

    fn alloc(&mut self) -> io::Result<PageId> {
        let id = self.pages;
        self.file.write_all_at(&[0u8; PAGE_SIZE], offset_of(id))?;
        self.pages += 1;
        Ok(id)
    }

    fn len(&self) -> u32 {
        self.pages
    }

    fn sync(&mut self) -> io::Result<()> {
        self.file.sync_all()
    }
}

#[derive(Default)]
pub struct MemDisk {
    pages: Vec<PageBuf>,
    pub reads: usize,
    pub writes: usize,
}

impl Disk for MemDisk {
    fn read(&mut self, id: PageId, buf: &mut PageBuf) -> io::Result<()> {
        buf.copy_from_slice(&self.pages[id as usize]);
        self.reads += 1;
        Ok(())
    }

    fn write(&mut self, id: PageId, buf: &PageBuf) -> io::Result<()> {
        self.pages[id as usize].copy_from_slice(buf);
        self.writes += 1;
        Ok(())
    }

    fn alloc(&mut self) -> io::Result<PageId> {
        let id = self.pages.len();
        self.pages.push([0; PAGE_SIZE]);
        Ok(id as u32)
    }

    fn len(&self) -> u32 {
        self.pages.len() as u32
    }

    fn sync(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl std::fmt::Debug for MemDisk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemDisk")
            .field("pages", &self.pages.len())
            .field("reads", &self.reads)
            .field("writes", &self.writes)
            .finish()
    }
}
