/**
A page of variable-length records, addressed by stable slot numbers.

This is a zero-cost view over a byte slice, in the same way `str` is a view over `[u8]`. It is
never owned: you only ever hold a `&SlottedPage` or a `&mut SlottedPage`, obtained from
[`open`](Self::open), [`open_mut`](Self::open_mut) or [`init`](Self::init).

The page knows nothing about what the records mean, and it does not interpret `page_kind` or
`next_page`. That belongs to the role that owns the page, such as a heap page or a B+tree node.

# Layout

All integers are big-endian.

| offset | size | field
| ------ | ---- | --------------------------------------------------------
|  0     | u8   | page_kind  - role tag, checked by the owning layer      |
|  1     | u8   | flags      - reserved, currently unused                 |
|  2     | u16  | slot_count - slots in the array, live and dead          |
|  4     | u16  | free_end   - where the record area begins               |
|  6     | u16  | garbage    - bytes held by dead or shrunk records       |
|  8     | u64  | lsn        - last WAL record applied to this page       |
| 16     | u32  | next_page  - link to the next page in a chain, 0 = none |
| 20     | ...  | slot array - (offset: u16, len: u16) per slot, grows up |
| ...    | ...  | free space -                                            |
| ...    | ...  | records    - grow down from the end of the page         |

```txt
+--------+------------------+----------------+-----------------+
| header | slot array  ->   |   free space   |  <-  records    |
+--------+------------------+----------------+-----------------+
0        20                                   free_end      len
```

# Invariants

- `HDR_LEN + slot_count * SLOT_LEN <= free_end <= buf.len()`. `open` checks this, so a corrupt
page is an error and not a panic.
- A slot with `offset == 0` is dead. Offset 0 lies inside the header, so it can never be a real
record location.
- Slot numbers are stable. Compaction moves record bytes but never renumbers slots, so a
`(page, slot)` pair stays valid until its record is deleted. A deleted slot may later be reused
by an insert.
- `garbage` is exactly the space that compaction would recover.

# Space

Free space comes in two kinds. *Contiguous* free space is the gap in the middle, and *reclaimable*
free space is that gap plus `garbage`. Inserts and updates compact automatically when the record
fits only in the reclaimable total.
*/
#[repr(transparent)]
pub struct SlottedPage {
    buf: [u8],
}

/// Index into a page's slot array.
pub type SlotId = u16;

const HDR_LEN: usize = 20;
const SLOT_LEN: usize = 4;

// Offsets
const KIND: usize = 0;
const SLOT_CNT: usize = 2;
const FREE_END: usize = 4;
const GARBAGE: usize = 6;
const NEXT: usize = 16;

use super::{Corrupt, PageId};
use crate::bytes::{Buf, BufMut};

struct Slot {
    off: usize,
    len: usize,
}

impl SlottedPage {
    pub fn open(buf: &[u8]) -> Result<&Self, Corrupt> {
        Self::validate(buf)?;
        Ok(Self::wrap(buf))
    }

    pub fn open_mut(buf: &mut [u8]) -> Result<&mut Self, Corrupt> {
        Self::validate(buf)?;
        Ok(Self::wrap_mut(buf))
    }

    pub fn init(buf: &mut [u8], kind: u8) -> &mut Self {
        let len = buf.len();
        assert!(len >= HDR_LEN && len <= u16::MAX as usize);

        buf[..HDR_LEN].fill(0);
        buf[KIND] = kind;
        buf.put_u16(FREE_END, len as u16);

        Self::wrap_mut(buf)
    }

    fn wrap(buf: &[u8]) -> &Self {
        // SAFETY: sound beacuse of `repr(transparent)` so the layout and pointer are identical
        unsafe { &*(buf as *const _ as *const Self) }
    }

    fn wrap_mut(buf: &mut [u8]) -> &mut Self {
        // SAFETY: same as above
        unsafe { &mut *(buf as *mut _ as *mut Self) }
    }

    fn validate(buf: &[u8]) -> Result<(), Corrupt> {
        if buf.len() < HDR_LEN || buf.len() > u16::MAX as usize {
            bail!(Corrupt)
        }

        let slots_end = HDR_LEN + buf.get_u16(SLOT_CNT) as usize * SLOT_LEN;
        let free_end = buf.get_u16(FREE_END) as usize;

        if free_end > buf.len() || slots_end > free_end {
            bail!(Corrupt)
        }

        Ok(())
    }

    pub fn kind(&self) -> u8 {
        self.buf[KIND]
    }

    /// Get the number of slots
    pub fn len(&self) -> u16 {
        self.buf.get_u16(SLOT_CNT)
    }

    fn free_end(&self) -> usize {
        self.buf.get_u16(FREE_END) as usize
    }

    fn garbage(&self) -> usize {
        self.buf.get_u16(GARBAGE) as usize
    }

    fn add_garbage(&mut self, n: usize) {
        let g = self.garbage() + n;
        self.buf.put_u16(GARBAGE, g as u16);
    }

    fn slot(&self, i: SlotId) -> Slot {
        let base = HDR_LEN + i as usize * SLOT_LEN;
        Slot {
            off: self.buf.get_u16(base) as usize,
            len: self.buf.get_u16(base + 2) as usize,
        }
    }

    fn set_slot(&mut self, i: SlotId, off: usize, len: usize) {
        let base = HDR_LEN + i as usize * SLOT_LEN;
        self.buf.put_u16(base, off as u16);
        self.buf.put_u16(base + 2, len as u16);
    }

    pub fn next(&self) -> PageId {
        self.buf.get_u32(NEXT)
    }

    pub fn set_next(&mut self, id: PageId) {
        self.buf.put_u32(NEXT, id)
    }

    /// Gap between the slot array and the content
    fn contiguous_free(&self) -> usize {
        let slots_end = HDR_LEN + self.len() as usize * SLOT_LEN;
        debug_assert!(slots_end <= self.free_end(), "slot array overlaps content");
        self.free_end() - slots_end
    }

    pub fn reclaimable_free(&self) -> usize {
        self.contiguous_free() + self.garbage()
    }

    /// Would `insert` of `len` bytes succeed? Read-only so usable under a shared latch
    pub fn can_insert(&self, len: usize) -> bool {
        let has_dead = (0..self.len()).any(|i| self.slot(i).off == 0);
        let need = len + if has_dead { 0 } else { SLOT_LEN };
        need <= self.reclaimable_free()
    }

    #[rustfmt::skip]
    pub fn get(&self, slot: SlotId) -> Option<&[u8]> {
        if slot >= self.len() { return None; }
        let Slot { off, len } = self.slot(slot);
        if off == 0 { None } else { Some(&self.buf[off..off + len]) }
    }

    /// Live records as (slotid, bytes), in slot order
    pub fn iter(&self) -> impl Iterator<Item = (SlotId, &[u8])> {
        (0..self.len()).filter_map(|i| self.get(i).map(|r| (i, r)))
    }

    /// Write `data` at the end of the free gap. Caller guarantees it fits
    fn write(&mut self, data: &[u8]) -> usize {
        let off = self.free_end() - data.len();
        self.buf.put_slice(off, data);
        self.buf.put_u16(FREE_END, off as u16);
        off
    }

    #[rustfmt::skip]
    pub fn insert(&mut self, data: &[u8]) -> Option<SlotId> {
        let reuse = (0..self.len()).find(|&i| self.slot(i).off == 0);
        let need = data.len() + if reuse.is_some() { 0 } else { SLOT_LEN };


        if need > self.reclaimable_free() { return None; }
        if need > self.contiguous_free() { self.compact(); }

        let off = self.write(data);
        let slot = reuse.unwrap_or_else(|| {
            let i = self.len();
            self.buf.put_u16(SLOT_CNT, i + 1);
            i
        });

        self.set_slot(slot, off, data.len());
        Some(slot)
    }

    pub fn delete(&mut self, slot: SlotId) {
        assert!(slot < self.len());
        let Slot { off, len } = self.slot(slot);
        assert!(off != 0);
        self.set_slot(slot, 0, 0);
        self.add_garbage(len);
    }

    /// Replace a record, keeping its slot id (so RIDs stay valid).
    /// Returns false if it can't fit in this page; the old record is then untouched.
    #[rustfmt::skip]
    pub fn update(&mut self, slot: SlotId, data: &[u8]) -> bool {
        assert!(slot < self.len());
        let Slot { off, len } = self.slot(slot);
        assert!(off != 0);

        if data.len() <= len {
            self.buf.put_slice(off, data);
            self.set_slot(slot, off, data.len());
            self.add_garbage(len - data.len());
            return true;
        }

        if data.len() > self.reclaimable_free() + len { return false; }

        // free the old copy, then place the new one
        self.set_slot(slot, 0, 0);
        self.add_garbage(len);
        if data.len() > self.contiguous_free() { self.compact(); }

        let new_off = self.write(data);
        self.set_slot(slot, new_off, data.len());
        true
    }

    /// Repack live cells against the end of the page. Slot ids do not change
    fn compact(&mut self) {
        let mut live: Vec<(u16, usize, usize)> = (0..self.len())
            .filter_map(|i| {
                let Slot { off, len } = self.slot(i);
                (off != 0).then_some((i, off, len))
            })
            .collect();

        // Highest offset first so records only move towards the end and never
        // overwrite one we haven't moved yet
        live.sort_by_key(|&(_, off, _)| std::cmp::Reverse(off));

        let mut cursor = self.buf.len();
        for (i, off, len) in live {
            cursor -= len;
            // only move a record if its destination is not its current location
            if off != cursor {
                self.buf.copy_within(off..off + len, cursor);
                self.set_slot(i, cursor, len);
            }
        }

        self.buf.put_u16(FREE_END, cursor as u16);
        self.buf.put_u16(GARBAGE, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foo() {
        const N: usize = 32;
        let mut buf = [0u8; N];

        let p = SlottedPage::init(&mut buf, 1);
        let a = dbg!(p.insert(b"ab").unwrap());
        dbg!(p.contiguous_free());
        let b = dbg!(p.insert(b"cd").unwrap());
        dbg!(p.contiguous_free());
        p.delete(b);
        dbg!(p.garbage(), p.contiguous_free(), p.reclaimable_free());
        dbg!(p.insert(b"cd").unwrap());
    }
}
