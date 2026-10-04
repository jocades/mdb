#![allow(unused)]

use crate::bytes::{Buf, BufMut};

/// A slotted page
///
/// Header:
///
/// ```txt
/// 0       u8   page_kind
/// 1       u8   flags (unused, for future overflow and b+tree metadata)
/// 2..4    u16  slot_count
/// 4..6    u16  free_end      (start of record area)
/// 6..8    u16  garbage       (bytes in dead/shrunk records, reclaimable by compaction)
/// 8..16   u64  lsn
/// 16..20  u32  next_page
/// 20..    slot array (offset u16, len u16 each) grows up; records grow down from the end
/// ```
#[repr(transparent)]
struct SlottedPage {
    buf: [u8],
}

const HDR_LEN: usize = 20;
const SLOT_LEN: usize = 4;

const KIND: usize = 0;
const SLOT_CNT: usize = 2;
const FREE_END: usize = 4;
const GARBAGE: usize = 6;
const NEXT: usize = 16;

struct Slot {
    off: usize,
    len: usize,
}

#[derive(Debug)]
struct Corrupt;

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

    // -- READS --

    fn kind(&self) -> u8 {
        self.buf[KIND]
    }

    /// Get the number of slots
    fn len(&self) -> u16 {
        self.buf.get_u16(SLOT_CNT)
    }

    fn free_end(&self) -> usize {
        self.buf.get_u16(FREE_END) as usize
    }

    fn garbage(&self) -> usize {
        self.buf.get_u16(GARBAGE) as usize
    }

    fn slot(&self, i: u16) -> Slot {
        let base = HDR_LEN + i as usize * SLOT_LEN;
        Slot {
            off: self.buf.get_u16(base) as usize,
            len: self.buf.get_u16(base + 2) as usize,
        }
    }

    /// Gap between the slot array and the content
    fn contiguous_free(&self) -> usize {
        let slots_end = (HDR_LEN + self.len() as usize * SLOT_LEN);
        debug_assert!(slots_end <= self.free_end(), "slot array overlaps content");
        self.free_end() - slots_end
    }

    /// Free bytes after compaction
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
    pub fn get(&self, slot: u16) -> Option<&[u8]> {
        if slot >= self.len() { return None; }
        let Slot { off, len } = self.slot(slot);
        if off == 0 { None } else { Some(&self.buf[off..off + len]) }
    }

    /// Live records as (slotid, bytes), in slot order
    pub fn iter(&self) -> impl Iterator<Item = (u16, &[u8])> {
        (0..self.len()).filter_map(|i| self.get(i).map(|r| (i, r)))
    }

    // -- WRITES --

    fn set_slot(&mut self, i: u16, off: usize, len: usize) {
        let base = HDR_LEN + i as usize * SLOT_LEN;
        self.buf.put_u16(base, off as u16);
        self.buf.put_u16(base + 2, len as u16);
    }

    fn add_garbage(&mut self, n: usize) {
        let g = self.garbage() + n;
        self.buf.put_u16(GARBAGE, g as u16);
    }

    /// Write `data` at the end of the free gap. Caller guarantees it fits
    fn write(&mut self, data: &[u8]) -> usize {
        let off = self.free_end() - data.len();
        self.buf[off..off + data.len()].copy_from_slice(data);
        self.buf.put_u16(FREE_END, off as u16);
        off
    }

    #[rustfmt::skip]
    fn insert(&mut self, data: &[u8]) -> Option<u16> {
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

    #[rustfmt::skip]
    fn delete(&mut self, slot: u16) -> bool {
        if slot >= self.len() { return false; }
        let Slot { off, len } = self.slot(slot);
        if off == 0 { return false; }
        self.set_slot(slot, 0, 0);
        self.add_garbage(len);
        true
    }

    /// Replace a record, keeping its slot id (so RIDs stay valid).
    /// Returns false if it can't fit in this page; the old record is then untouched.
    #[rustfmt::skip]
    fn update(&mut self, slot: u16, data: &[u8]) -> bool {
        if slot >= self.len() { return false; }
        let Slot { off, len } = self.slot(slot);
        if off == 0 { return false; }

        if data.len() <= len {
            // shrink of same size: overwrite in place
            self.buf[off..off + data.len()].copy_from_slice(data);
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

        let mut end = self.buf.len();
        for (i, off, len) in live {
            end -= len;
            self.buf.copy_within(off..off + len, end);
            self.set_slot(i, end, len);
        }

        self.buf.put_u16(FREE_END, end as u16);
        self.buf.put_u16(GARBAGE, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: usize = 512;

    fn init(buf: &mut [u8]) -> &mut SlottedPage {
        SlottedPage::init(buf, 1)
    }

    fn content(p: &SlottedPage) -> Vec<(u16, Vec<u8>)> {
        // a bit dirty but we need owned data for multiple asserts
        p.iter().map(|(s, r)| (s, r.to_vec())).collect()
    }

    fn rec(len: usize, tag: u8) -> Vec<u8> {
        vec![tag; len]
    }

    /// Insert `len`-byte records until the page is full. Record `i` is filled with the byte `i`,
    /// so every record is distinguishable
    fn fill(p: &mut SlottedPage, len: usize) -> Vec<u16> {
        let mut slots = Vec::new();
        while let Some(s) = p.insert(&rec(len, slots.len() as u8)) {
            slots.push(s)
        }
        slots
    }

    // -- BASICS --

    #[test]
    fn init_is_empty() {
        let mut buf = [0xffu8; N];
        let p = SlottedPage::init(&mut buf, 3);
        assert_eq!(p.kind(), 3);
        assert_eq!(p.len(), 0);
        assert_eq!(p.reclaimable_free(), N - HDR_LEN);
        assert_eq!(p.iter().count(), 0);
        assert!(p.get(0).is_none())
    }

    #[test]
    fn insert_and_get() {
        let mut buf = [0u8; N];
        let p = init(&mut buf);
        let a = p.insert(b"alpha").unwrap();
        let b = p.insert(b"beta").unwrap();
        let c = p.insert(b"gamma").unwrap();
        assert_eq!((a, b, c), (0, 1, 2));
        assert_eq!(p.get(a), Some(&b"alpha"[..]));
        assert_eq!(p.get(b), Some(&b"beta"[..]));
        assert_eq!(p.get(c), Some(&b"gamma"[..]));
        assert_eq!(p.get(3), None);
        assert_eq!(p.len(), 3);
    }

    #[test]
    fn empty_record_is_a_live_record() {
        let mut buf = [0u8; N];
        let p = init(&mut buf);
        let s = p.insert(b"").unwrap();
        assert_eq!(p.get(s), Some(&b""[..]));
        assert_eq!(p.iter().count(), 1);
        assert!(p.delete(s));
        assert_eq!(p.get(s), None);
    }

    #[test]
    fn iter_skips_dead_slots_in_slot_order() {
        let mut buf = [0u8; N];
        let p = init(&mut buf);
        for x in 0..5u8 {
            p.insert(&[x]).unwrap();
        }
        p.delete(1);
        p.delete(3);
        assert_eq!(content(p), vec![(0, vec![0]), (2, vec![2]), (4, vec![4])]);
    }

    // -- CAPACITY --

    #[test]
    fn fills_to_exactly_the_computed_capacity() {
        let mut buf = [0u8; N];
        let p = init(&mut buf);
        let slots = fill(p, 10);
        assert_eq!(slots.len(), (N - HDR_LEN) / (SLOT_LEN + 10)); // 35
        for s in slots {
            assert_eq!(p.get(s), Some(rec(10, s as u8).as_slice()));
        }
    }

    #[test]
    fn largest_possible_record_fits_and_one_more_byte_does_not() {
        let max = N - HDR_LEN - SLOT_LEN; // 492
        let mut buf = [0u8; N];
        let p = init(&mut buf);
        assert!(p.can_insert(max));
        assert!(!p.can_insert(max + 1));
        assert!(p.insert(&rec(max + 1, 1)).is_none());
        let s = p.insert(&rec(max, 2)).unwrap();
        assert_eq!(p.get(s), Some(rec(max, 2).as_slice()));
        assert_eq!(p.reclaimable_free(), 0);
    }

    #[test]
    fn failed_insert_leaves_page_unchanged() {
        let mut buf = [0u8; N];
        let p = init(&mut buf);
        fill(p, 10);
        let before = content(p);
        let free = p.reclaimable_free();
        assert!(p.insert(&rec(10, 0xaa)).is_none());
        assert_eq!(content(p), before);
        assert_eq!(p.reclaimable_free(), free);
    }

    #[test]
    fn works_with_a_4k_page() {
        let mut buf = [0u8; 4096];
        let p = init(&mut buf);
        let slots = fill(p, 100);
        assert_eq!(slots.len(), (4096 - HDR_LEN) / (SLOT_LEN + 100));
        for s in slots {
            assert_eq!(p.get(s), Some(rec(100, s as u8).as_slice()));
        }
    }

    // -- DELETE --

    #[test]
    fn delete_removes_only_that_record() {
        let mut buf = [0u8; N];
        let p = init(&mut buf);
        for i in 0..3u8 {
            p.insert(&rec(5, i)).unwrap();
        }
        assert!(p.delete(1));
        assert_eq!(p.get(1), None);
        assert_eq!(p.get(0), Some(rec(5, 0).as_slice()));
        assert_eq!(p.get(2), Some(rec(5, 2).as_slice()));
        assert_eq!(p.len(), 3); // the slot array never shrinks
    }

    #[test]
    fn delete_invalid_slots_returns_false() {
        let mut buf = [0u8; N];
        let p = init(&mut buf);
        let s = p.insert(b"x").unwrap();
        assert!(p.delete(s));
        assert!(!p.delete(s)); // already dead
        assert!(!p.delete(99)); // out of range
    }

    #[test]
    fn insert_reuses_the_first_dead_slot() {
        let mut buf = [0u8; N];
        let p = init(&mut buf);
        for i in 0..3u8 {
            p.insert(&[i]).unwrap();
        }
        p.delete(1);
        assert_eq!(p.insert(b"new"), Some(1));
        assert_eq!(p.len(), 3);
        assert_eq!(p.get(1), Some(&b"new"[..]));
    }

    // -- COMPACTION --

    #[test]
    fn insert_compacts_when_space_is_fragmented() {
        let mut buf = [0u8; N];
        let p = init(&mut buf);
        fill(p, 10); // 35 records, 6 bytes of contiguous space left
        assert_eq!(p.contiguous_free(), 6);

        for s in (0..35u16).step_by(2) {
            assert!(p.delete(s)); // 18 deletes, 180 bytes of garbage
        }
        assert_eq!(p.contiguous_free(), 6); // deleting doesn't move anything
        assert_eq!(p.reclaimable_free(), 6 + 180);

        // can_insert must predict the boundary exactly
        assert!(p.can_insert(186));
        assert!(!p.can_insert(187));

        let new = p.insert(&rec(100, 0xee)).unwrap(); // only fits after compaction
        assert_eq!(new, 0); // reused the first dead slot
        assert_eq!(p.get(new), Some(rec(100, 0xee).as_slice()));
        assert_eq!(p.reclaimable_free(), 86);

        // survivors keep their slot ids and contents
        for s in (1..35u16).step_by(2) {
            assert_eq!(p.get(s), Some(rec(10, s as u8).as_slice()));
        }
    }
}
