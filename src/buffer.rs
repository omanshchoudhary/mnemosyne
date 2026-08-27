// TODO: drop once BufferPool has its methods.
#![allow(dead_code)]

pub mod replacer;

use std::{collections::HashMap, sync::atomic::AtomicU64};

use crate::{
    disk::DiskManager,
    page::{Page, PageId},
};

use replacer::LruReplacer;

pub type FrameId = usize;

pub struct BufferPool {
    frames: Vec<Frame>,
    page_table: HashMap<PageId, FrameId>,
    free_list: Vec<FrameId>,
    replacer: LruReplacer,
    disk: DiskManager,
    hits: AtomicU64,
    misses: AtomicU64,
}
struct Frame {
    page: Page,
    page_id: Option<PageId>,
    pin_count: u32,
    is_dirty: bool,
}
impl Frame {
    fn empty() -> Self {
        Self {
            page: Page::new(),
            page_id: None,
            pin_count: 0,
            is_dirty: false,
        }
    }
    fn reset(&mut self, page_id: PageId, page: Page) {
        self.page = page;
        self.page_id = Some(page_id);
        self.pin_count = 0;
        self.is_dirty = false;
    }
    fn is_pinned(&self) -> bool {
        self.pin_count > 0
    }
}
