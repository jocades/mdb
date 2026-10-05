use derive_more::{Deref, DerefMut};

use crate::storage::{Corrupt, PageView, SlottedPage, page_kind};

#[repr(transparent)]
#[derive(Deref, DerefMut)]
pub struct HeapPage(SlottedPage);

impl PageView for HeapPage {
    fn open(buf: &[u8]) -> Result<&Self, Corrupt> {
        let p = SlottedPage::open(buf)?;
        ensure!(p.kind() == page_kind::HEAP, Corrupt);
        Ok(unsafe { &*(p as *const _ as *const Self) })
    }

    fn open_mut(buf: &mut [u8]) -> Result<&mut Self, Corrupt> {
        let p = SlottedPage::open_mut(buf)?;
        ensure!(p.kind() == page_kind::HEAP, Corrupt);
        Ok(unsafe { &mut *(p as *mut _ as *mut Self) })
    }
}

impl HeapPage {
    pub fn init(buf: &mut [u8]) -> &mut Self {
        let p = SlottedPage::init(buf, page_kind::HEAP);
        unsafe { &mut *(p as *mut _ as *mut Self) }
    }
}
