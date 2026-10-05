use std::io;

use derive_more::{Display, From};

use super::page::HeapPage;
use crate::storage::{Corrupt, Disk, PageId, Pool, SlotId};

/// The `ROW ID`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, From)]
#[from((PageId, SlotId))]
#[display("({page},{slot})")]
pub struct Rid {
    pub page: PageId,
    pub slot: SlotId,
}

impl Rid {
    pub fn new(page: PageId, slot: SlotId) -> Self {
        Self { page, slot }
    }
}

#[derive(Debug, From)]
pub enum Error {
    NotFound(Rid),
    RowTooLarge {
        len: usize,
        max: usize,
    },
    #[from(io::Error, Corrupt)]
    Io(io::Error),
}

/// A table represented as a chain of pages in no particular order.
pub struct HeapTable {
    first: PageId,
    last: PageId,
}

impl HeapTable {
    /// New empty table
    pub fn create(pool: &mut Pool<impl Disk>) -> io::Result<Self> {
        let id = alloc_page_and_init(pool)?;
        Ok(Self {
            first: id,
            last: id,
        })
    }

    /// Open an existing table by walking the chain once to find the last page.
    #[rustfmt::skip]
    pub fn open(pool: &mut Pool<impl Disk>, first: PageId) -> io::Result<Self> {
        let mut current = first;
        loop {
            let next = pool.get::<HeapPage>(current)?.next();
            if next == 0 { break; }
            current = next;
        }
        Ok(Self {
            first,
            last: current,
        })
    }

    pub fn get<'a>(&self, pool: &'a mut Pool<impl Disk>, rid: Rid) -> io::Result<Option<&'a [u8]>> {
        Ok(pool.get::<HeapPage>(rid.page)?.get(rid.slot))
    }

    pub fn scan(&self) -> HeapScan {
        HeapScan(Rid::new(self.first, 0))
    }

    pub fn insert(&mut self, pool: &mut Pool<impl Disk>, row: &[u8]) -> Result<Rid, Error> {
        const MAX_ROW: usize = 100;

        if row.len() > MAX_ROW {
            bail!(Error::RowTooLarge {
                len: row.len(),
                max: MAX_ROW
            })
        }

        if let Some(slot) = try_insert(pool, self.last, row)? {
            return Ok(Rid::new(self.last, slot));
        }

        // last page is full: allocate, link, then insert into the fresh page
        let new = alloc_page_and_init(pool)?;
        let old = self.last;
        pool.get_mut::<HeapPage>(old)?.set_next(new);
        self.last = new;

        // can't fail: row <= MAX_ROW
        let slot = try_insert(pool, new, row)?.ok_or(Corrupt)?;
        Ok(Rid::new(new, slot))
    }
}

fn try_insert(pool: &mut Pool<impl Disk>, id: PageId, row: &[u8]) -> io::Result<Option<SlotId>> {
    if !pool.get::<HeapPage>(id)?.can_insert(row.len()) {
        return Ok(None);
    }
    Ok(pool.get_mut::<HeapPage>(id)?.insert(row))
}

fn alloc_page_and_init(pool: &mut Pool<impl Disk>) -> io::Result<PageId> {
    let id = pool.alloc()?;
    HeapPage::init(pool.writer(id)?);
    Ok(id)
}

pub struct HeapScan(Rid);

impl HeapScan {
    pub fn next<'a>(
        &mut self,
        pool: &'a mut Pool<impl Disk>,
    ) -> io::Result<Option<(Rid, &'a [u8])>> {
        while self.0.page != 0 {
            let start = self.0.slot;
            let page = pool.get::<HeapPage>(self.0.page)?;

            // let found = (start..page.len()).find_map(|slot| page.get(slot).map(|row| (slot, row)));

            let mut found = None;
            for slot in start..page.len() {
                if let Some(row) = page.get(slot) {
                    found = Some((slot, row as *const [u8]));
                    break;
                }
            }

            match found {
                Some((slot, row)) => {
                    self.0.slot = slot + 1;
                    // SAFETY: only this branch of the loop returns `row` but the compiler does not
                    // know that, so it forces the borrow of `pool.get()` to last for `'a`, it then
                    // applies the long lifetime to every iteration, including ones that dont return,
                    // so the next iteration's `pool.get()` looks like a second mutable borrow,
                    // which is incorrect.
                    return Ok(Some((Rid::new(self.0.page, slot), unsafe { &*row })));
                }
                None => {
                    self.0.page = page.next();
                    self.0.slot = 0;
                }
            }
        }
        Ok(None)
    }
}
