#![allow(dead_code)]

use crate::page::{Page, PageId};

// 0 reserved for MetaPage
pub(crate) const META_PAGE_ID: PageId = PageId(0);

// spells "MNMS", so a file that is not ours fails on open instead of later
const META_MAGIC: u32 = 0x4D4E_4D53;
const META_VERSION: u32 = 1;

const OFF_MAGIC: usize = 0; // u32
const OFF_VERSION: usize = 4; // u32
const OFF_ROOT: usize = 8; // u64 - B+tree root page id
const OFF_FREE_LIST: usize = 16; // u64 - head of the free page list, later
const OFF_HEAP_TAIL: usize = 24; // u64 - heap page new records go to
const OFF_NEXT_TIMESTAMP: usize = 32; // u64 - timestamp the next transaction gets

impl Page {
    // meta page creation when db file is created
    pub(crate) fn init_meta(&mut self, root: PageId) {
        self.write_u32(OFF_MAGIC, META_MAGIC);
        self.write_u32(OFF_VERSION, META_VERSION);
        self.write_u64(OFF_FREE_LIST, 0);
        self.write_u64(OFF_HEAP_TAIL, 0);
        self.write_u64(OFF_NEXT_TIMESTAMP, 0);
        self.set_root_page_id(root);
    }

    pub(crate) fn is_meta(&self) -> bool {
        self.read_u32(OFF_MAGIC) == META_MAGIC && self.read_u32(OFF_VERSION) == META_VERSION
    }

    pub(crate) fn root_page_id(&self) -> PageId {
        PageId(self.read_u64(OFF_ROOT))
    }

    pub(crate) fn set_root_page_id(&mut self, root: PageId) {
        self.write_u64(OFF_ROOT, root.0);
    }

    pub(crate) fn heap_tail(&self) -> Option<PageId> {
        match self.read_u64(OFF_HEAP_TAIL) {
            0 => None,
            raw => Some(PageId(raw)),
        }
    }

    pub(crate) fn set_heap_tail(&mut self, tail: PageId) {
        self.write_u64(OFF_HEAP_TAIL, tail.0);
    }
    pub(crate) fn next_timestamp(&self) -> u64 {
        self.read_u64(OFF_NEXT_TIMESTAMP)
    }

    pub(crate) fn set_next_timestamp(&mut self, timestamp: u64) {
        self.write_u64(OFF_NEXT_TIMESTAMP, timestamp);
    }
}

#[cfg(test)]
#[path = "meta_tests.rs"]
mod tests;
