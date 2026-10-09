use std::{fs::File, io, os::unix::fs::FileExt, path::Path};

use super::{PAGE_SIZE, PageBuf, PageId};

/// Page I/O
pub trait Disk {
    fn read(&mut self, id: PageId, buf: &mut PageBuf) -> io::Result<()>;
    fn write(&mut self, id: PageId, buf: &PageBuf) -> io::Result<()>;
    fn alloc(&mut self) -> io::Result<PageId>;
    fn sync(&mut self) -> io::Result<()>;
    fn len(&self) -> u32;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[inline]
fn offset_of(id: PageId) -> u64 {
    id as u64 * PAGE_SIZE as u64
}

fn corrupt(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[derive(Debug)]
pub struct FileDisk {
    file: File,
    pages: u32,
}

impl FileDisk {
    pub fn with_file(file: File) -> io::Result<Self> {
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

    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::options()
            .read(true)
            .create(true)
            .write(true)
            .open(path)?;
        Self::with_file(file)
    }

    #[cfg(test)]
    pub fn temp() -> io::Result<Self> {
        let file = tempfile::tempfile()?;
        Self::with_file(file)
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
