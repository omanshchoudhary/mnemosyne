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
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_db() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        (dir, path)
    }

    #[test]
    fn new_page_hands_out_ascending_ids() {
        let (_dir, path) = temp_db();
        let mut pool = BufferPool::open(&path, 4).unwrap();

        let (first, _) = pool.new_page().unwrap();
        let (second, _) = pool.new_page().unwrap();

        assert_eq!(first, PageId(0));
        assert_eq!(second, PageId(1));
    }

    #[test]
    fn a_written_page_reads_back_through_the_pool() {
        let (_dir, path) = temp_db();
        let mut pool = BufferPool::open(&path, 4).unwrap();

        let (page_id, frame_id) = pool.new_page().unwrap();
        pool.page_mut(frame_id).write_u64(0, 0xCAFE);
        pool.unpin(frame_id).unwrap();

        let again = pool.fetch_page(page_id).unwrap();
        assert_eq!(pool.page(again).read_u64(0), 0xCAFE);
    }

    #[test]
    fn fetching_a_cached_page_reuses_its_frame() {
        let (_dir, path) = temp_db();
        let mut pool = BufferPool::open(&path, 4).unwrap();

        let (page_id, frame_id) = pool.new_page().unwrap();
        pool.unpin(frame_id).unwrap();

        assert_eq!(pool.fetch_page(page_id).unwrap(), frame_id);
        assert_eq!(pool.free_list.len(), 3);
    }

    #[test]
    fn a_dirty_page_survives_eviction() {
        let (_dir, path) = temp_db();
        let mut pool = BufferPool::open(&path, 2).unwrap();

        let (first, frame_id) = pool.new_page().unwrap();
        pool.page_mut(frame_id).write_u64(0, 42);
        pool.unpin(frame_id).unwrap();

        for _ in 0..2 {
            let (_, frame_id) = pool.new_page().unwrap();
            pool.unpin(frame_id).unwrap();
        }

        assert!(!pool.page_table.contains_key(&first));

        let frame_id = pool.fetch_page(first).unwrap();
        assert_eq!(pool.page(frame_id).read_u64(0), 42);
    }

    #[test]
    fn a_new_page_never_inherits_the_old_frames_bytes() {
        let (_dir, path) = temp_db();
        let mut pool = BufferPool::open(&path, 1).unwrap();

        let (_, frame_id) = pool.new_page().unwrap();
        pool.page_mut(frame_id).write_u64(0, 0xFFFF);
        pool.unpin(frame_id).unwrap();

        let (_, frame_id) = pool.new_page().unwrap();
        assert_eq!(pool.page(frame_id).read_u64(0), 0);
    }

    #[test]
    fn a_fully_pinned_pool_cannot_take_another_page() {
        let (_dir, path) = temp_db();
        let mut pool = BufferPool::open(&path, 2).unwrap();

        pool.new_page().unwrap();
        pool.new_page().unwrap();

        assert!(matches!(pool.new_page(), Err(Error::BufferPoolFull)));
    }

    #[test]
    fn unpinning_lets_a_frame_be_reused() {
        let (_dir, path) = temp_db();
        let mut pool = BufferPool::open(&path, 1).unwrap();

        let (_, frame_id) = pool.new_page().unwrap();
        assert!(matches!(pool.new_page(), Err(Error::BufferPoolFull)));

        pool.unpin(frame_id).unwrap();
        assert!(pool.new_page().is_ok());
    }

    #[test]
    fn a_page_pinned_twice_needs_two_unpins() {
        let (_dir, path) = temp_db();
        let mut pool = BufferPool::open(&path, 1).unwrap();

        let (page_id, frame_id) = pool.new_page().unwrap();
        assert_eq!(pool.fetch_page(page_id).unwrap(), frame_id);

        pool.unpin(frame_id).unwrap();
        assert!(matches!(pool.new_page(), Err(Error::BufferPoolFull)));

        pool.unpin(frame_id).unwrap();
        assert!(pool.new_page().is_ok());
    }

    #[test]
    fn unpinning_twice_is_an_error() {
        let (_dir, path) = temp_db();
        let mut pool = BufferPool::open(&path, 2).unwrap();

        let (_, frame_id) = pool.new_page().unwrap();
        pool.unpin(frame_id).unwrap();

        assert!(matches!(
            pool.unpin(frame_id),
            Err(Error::FrameNotPinned(_))
        ));
    }

    #[test]
    fn flush_all_puts_everything_on_disk() {
        let (_dir, path) = temp_db();

        let page_id = {
            let mut pool = BufferPool::open(&path, 4).unwrap();
            let (page_id, frame_id) = pool.new_page().unwrap();
            pool.page_mut(frame_id).write_u64(0, 0xDEAD_BEEF);
            pool.unpin(frame_id).unwrap();
            pool.flush_all().unwrap();
            page_id
        };

        let mut pool = BufferPool::open(&path, 4).unwrap();
        let frame_id = pool.fetch_page(page_id).unwrap();
        assert_eq!(pool.page(frame_id).read_u64(0), 0xDEAD_BEEF);
    }

    #[test]
    fn dropping_the_pool_without_flushing_loses_the_change() {
        let (_dir, path) = temp_db();

        let page_id = {
            let mut pool = BufferPool::open(&path, 4).unwrap();
            let (page_id, frame_id) = pool.new_page().unwrap();
            pool.page_mut(frame_id).write_u64(0, 7);
            pool.unpin(frame_id).unwrap();
            page_id
        };

        let mut pool = BufferPool::open(&path, 4).unwrap();
        let frame_id = pool.fetch_page(page_id).unwrap();
        assert_eq!(pool.page(frame_id).read_u64(0), 0);
    }
}
