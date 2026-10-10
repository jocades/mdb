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

/**
An unordered collection of variable-length records, stored as a singly
linked chain of slotted pages.

"Heap" means there is no ordering: an insert goes into whichever page has
room (currently only the last one), and a record is found again by its
[`Rid`], not by any key. A full scan walks the chain from `first`.

The heap stores opaque bytes. It knows nothing about columns, types or
schemas: that meaning comes from the layer above (a table decodes records
with its schema). The same structure backs user tables and the catalog,
and could back anything else that is "a bag of records".

A `Heap` is just two page ids. It owns no pages and holds no reference to
the pool, which is passed into each call. `first` is the heap's permanent
identity (it is what the catalog stores) and is never freed while the heap
exists, even if it becomes empty.

Records larger than one page are not supported yet.
*/
#[derive(Debug)]
pub struct Heap {
    first: PageId,
    last: PageId,
}

impl Heap {
    /// New empty table
    pub fn create(pool: &mut Pool<impl Disk>) -> io::Result<Self> {
        let id = alloc_page_and_init(pool)?;
        Ok(Self {
            first: id,
            last: id,
        })
    }

    /// New empty table from an already allocated page, caller guarantees the page is valid
    // pub fn init(first: PageId) -> Self {
    //     HeapPage::init(first);
    //     Self { first, last: first }
    // }

    /// Open an existing table by walking the chain once to find the last page.
    /// This is **very ineficient** but it can be solved by storing both the first and last page
    /// in the catalog
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

    pub fn first(&self) -> PageId {
        self.first
    }

    pub fn get<'a>(&self, pool: &'a mut Pool<impl Disk>, rid: Rid) -> io::Result<Option<&'a [u8]>> {
        Ok(pool.get::<HeapPage>(rid.page)?.get(rid.slot))
    }

    pub fn scan(&self) -> HeapScan {
        HeapScan(Rid::new(self.first, 0))
    }

    // todo: overflow pages
    const MAX_SIZE: usize = 100;

    pub fn insert(&mut self, pool: &mut Pool<impl Disk>, record: &[u8]) -> io::Result<Rid> {
        if record.len() > Self::MAX_SIZE {
            unimplemented!("overflow pages")
        }

        if let Some(slot) = try_insert(pool, self.last, record)? {
            return Ok(Rid::new(self.last, slot));
        }

        // last page is full: allocate, link, then insert into the fresh page
        let new = alloc_page_and_init(pool)?;
        let old = self.last;
        pool.get_mut::<HeapPage>(old)?.set_next(new);
        self.last = new;

        // can't fail: row <= MAX_ROW
        let slot = try_insert(pool, new, record)?.ok_or(Corrupt)?;
        Ok(Rid::new(new, slot))
    }

    pub fn delete(&mut self, pool: &mut Pool<impl Disk>, rid: Rid) -> io::Result<()> {
        pool.get_mut::<HeapPage>(rid.page)?.delete(rid.slot);
        Ok(())
    }

    /// If the row no longer fits in its page it moves and new `Rid` is returned
    pub fn update(
        &mut self,
        pool: &mut Pool<impl Disk>,
        rid: Rid,
        record: &[u8],
    ) -> io::Result<Option<Rid>> {
        if record.len() > Self::MAX_SIZE {
            unimplemented!("overflow pages")
        }
        let page = pool.get_mut::<HeapPage>(rid.page)?;
        if page.update(rid.slot, record) {
            return Ok(None);
        }
        let new = self.insert(pool, record)?;
        self.delete(pool, rid)?;
        Ok(Some(new))
    }

    /// Return every page to the free list.
    /// Takes ownership of `self` so that it cannot be used afterwards
    pub fn destroy(self, pool: &mut Pool<impl Disk>) -> io::Result<()> {
        let mut current = self.first;
        while current != 0 {
            let next = pool.get::<HeapPage>(current)?.next();
            pool.free(current)?;
            current = next;
        }
        Ok(())
    }
}

fn try_insert(pool: &mut Pool<impl Disk>, id: PageId, record: &[u8]) -> io::Result<Option<SlotId>> {
    if !pool.get::<HeapPage>(id)?.can_insert(record.len()) {
        return Ok(None);
    }
    Ok(pool.get_mut::<HeapPage>(id)?.insert(record))
}

pub fn alloc_page_and_init(pool: &mut Pool<impl Disk>) -> io::Result<PageId> {
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
