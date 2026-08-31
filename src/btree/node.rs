#![allow(dead_code)]

use crate::page::{Page, PageId};

const PAGE_TYPE_LEAF: u8 = 2;
const PAGE_TYPE_INTERNAL: u8 = 3;

// Page 0 is the meta page
const NO_PAGE: u64 = 0;

impl Page {
    pub(crate) fn is_leaf(&self) -> bool {
        self.page_type() == PAGE_TYPE_LEAF
    }

    pub(crate) fn is_internal(&self) -> bool {
        self.page_type() == PAGE_TYPE_INTERNAL
    }

    pub(crate) fn next_leaf(&self) -> Option<PageId> {
        match self.link() {
            NO_PAGE => None,
            raw => Some(PageId(raw)),
        }
    }

    // the last leaf in the chain has no next
    pub(crate) fn set_next_leaf(&mut self, next: Option<PageId>) {
        self.set_link(next.map_or(NO_PAGE, |page_id| page_id.0));
    }

    pub(crate) fn rightmost_child(&self) -> Option<PageId> {
        match self.link() {
            NO_PAGE => None,
            raw => Some(PageId(raw)),
        }
    }

    pub(crate) fn set_rightmost_child(&mut self, child: PageId) {
        self.set_link(child.0);
    }
}
