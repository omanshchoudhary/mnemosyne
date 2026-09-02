mod frame;
pub mod replacer;

use std::collections::HashMap;
use std::path::Path;

use crate::{
    disk::DiskManager,
    error::{Error, Result},
    page::{Page, PageId},
};

use frame::Frame;
use replacer::LruReplacer;

pub type FrameId = usize;

pub struct BufferPool {
    frames: Vec<Frame>,
    page_table: HashMap<PageId, FrameId>,
    free_list: Vec<FrameId>,
    replacer: LruReplacer,
    disk: DiskManager,
}

impl BufferPool {
    pub fn open(path: &Path, frame_count: usize) -> Result<Self> {
        Ok(Self {
            frames: (0..frame_count).map(|_| Frame::empty()).collect(),
            page_table: HashMap::new(),
            free_list: (0..frame_count).rev().collect(),
            replacer: LruReplacer::new(frame_count),
            disk: DiskManager::open(path)?,
        })
    }

    pub fn fetch_page(&mut self, page_id: PageId) -> Result<FrameId> {
        // hit
        if let Some(&frame_id) = self.page_table.get(&page_id) {
            if !self.frames[frame_id].is_pinned() {
                self.replacer.remove(frame_id);
            }
            self.frames[frame_id].pin_count += 1;
            return Ok(frame_id);
        }

        // miss meaning first bring page from disk
        let page = self.disk.read_page(page_id)?;
        let frame_id = self.claim_frame()?;
        self.frames[frame_id].reset(page_id, page);
        self.page_table.insert(page_id, frame_id);
        self.frames[frame_id].pin_count += 1;
        Ok(frame_id)
    }

    pub fn new_page(&mut self) -> Result<(PageId, FrameId)> {
        let page_id = self.disk.allocate_page()?;
        let frame_id = self.claim_frame()?;
        // a recycled frame still holds the old page's bytes, so overwrite them
        self.frames[frame_id].reset(page_id, Page::new());
        self.page_table.insert(page_id, frame_id);
        self.frames[frame_id].pin_count += 1;
        Ok((page_id, frame_id))
    }

    // 0 means the file is brand new and holds no meta page yet
    pub fn page_count(&self) -> Result<u64> {
        self.disk.page_count()
    }

    pub fn page(&self, frame_id: FrameId) -> &Page {
        &self.frames[frame_id].page
    }

    pub fn page_mut(&mut self, frame_id: FrameId) -> &mut Page {
        self.frames[frame_id].is_dirty = true;
        &mut self.frames[frame_id].page
    }

    pub fn unpin(&mut self, frame_id: FrameId) -> Result<()> {
        if !self.frames[frame_id].is_pinned() {
            return Err(Error::FrameNotPinned(frame_id));
        }
        self.frames[frame_id].pin_count -= 1;

        if !self.frames[frame_id].is_pinned() {
            self.replacer.insert(frame_id);
        }
        Ok(())
    }

    // redirects to flush frame eventually
    pub fn flush_page(&mut self, page_id: PageId) -> Result<()> {
        let Some(&frame_id) = self.page_table.get(&page_id) else {
            return Ok(());
        };
        self.flush_frame(frame_id)
    }

    pub fn flush_all(&mut self) -> Result<()> {
        for frame_id in 0..self.frames.len() {
            self.flush_frame(frame_id)?;
        }
        // write into disk from os cache
        self.disk.sync()
    }

    fn claim_frame(&mut self) -> Result<FrameId> {
        match self.free_list.pop() {
            Some(frame_id) => Ok(frame_id),
            None => {
                if let Some(frame_id) = self.replacer.evict() {
                    if let Some(page_id) = self.frames[frame_id].page_id {
                        // flush first, flush_page finds the frame through the table
                        self.flush_page(page_id)?;
                        self.page_table.remove(&page_id);
                    }
                    Ok(frame_id)
                } else {
                    Err(Error::BufferPoolFull)
                }
            }
        }
    }

    fn flush_frame(&mut self, frame_id: FrameId) -> Result<()> {
        let frame = &mut self.frames[frame_id];

        let Some(page_id) = frame.page_id else {
            return Ok(());
        };

        if !frame.is_dirty {
            return Ok(());
        }

        // writes to OS Page Cache
        self.disk.write_page(page_id, &frame.page)?;

        frame.is_dirty = false;
        Ok(())
    }
}

#[cfg(test)]
#[path = "buffer_tests.rs"]
mod tests;
