use crate::page::{Page, PageId};

pub(super) struct Frame {
    pub(super) page: Page,
    pub(super) page_id: Option<PageId>,
    pub(super) pin_count: u32,
    pub(super) is_dirty: bool,
}

impl Frame {
    pub(super) fn empty() -> Self {
        Self {
            page: Page::new(),
            page_id: None,
            pin_count: 0,
            is_dirty: false,
        }
    }

    pub(super) fn reset(&mut self, page_id: PageId, page: Page) {
        self.page = page;
        self.page_id = Some(page_id);
        self.pin_count = 0;
        self.is_dirty = false;
    }

    pub(super) fn is_pinned(&self) -> bool {
        self.pin_count > 0
    }
}
