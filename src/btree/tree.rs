#![allow(dead_code)]

use std::path::Path;

use crate::buffer::{BufferPool, FrameId};
use crate::error::{Error, Result};
use crate::page::{PageId, RecordId};

// 0 reserved for MetaPage
const META_PAGE_ID: PageId = PageId(0);

pub struct BTree {
    pool: BufferPool,
}

impl BTree {
    pub fn open(path: &Path, frame_count: usize) -> Result<Self> {
        let mut pool = BufferPool::open(path, frame_count)?;

        if pool.page_count()? == 0 {
            // allocation is sequential from an empty file
            let (_meta_id, meta_frame) = pool.new_page()?; // page 0
            let (root_id, root_frame) = pool.new_page()?; // page 1

            pool.page_mut(root_frame).init_leaf();
            pool.page_mut(meta_frame).init_meta(root_id);

            pool.unpin(root_frame)?;
            pool.unpin(meta_frame)?;
            pool.flush_all()?;
        } else {
            let meta_frame = pool.fetch_page(META_PAGE_ID)?;
            let valid = pool.page(meta_frame).is_meta();
            pool.unpin(meta_frame)?;

            if !valid {
                return Err(Error::BadMetaPage);
            }
        }

        Ok(Self { pool })
    }

    fn root(&mut self) -> Result<PageId> {
        let frame = self.pool.fetch_page(META_PAGE_ID)?;
        let root =  self.pool.page(frame).root_page_id();
        self.pool.unpin(frame)?;
        Ok(root)
    }

    fn set_root(&mut self, root: PageId) -> Result<()> {
        let frame = self.pool.fetch_page(META_PAGE_ID)?;
        self.pool.page_mut(frame).set_root_page_id(root);
        self.pool.unpin(frame)?;
        Ok(())
    }

    fn find_leaf(&mut self, key: &[u8]) -> Result<FrameId> {
        
        let mut page_id = self.root()?;

        loop {
            let frame = self.pool.fetch_page(page_id)?;
            if self.pool.page(frame).is_leaf() {
                return Ok(frame);          // still pinned, on purpose
            }
            let child = self.pool.page(frame).child_for_key(key)?;
            self.pool.unpin(frame)?;
            page_id = child;
        }
    }

    pub fn lookup(&mut self, key: &[u8]) -> Result<Option<RecordId>> {
        let frame = self.find_leaf(key)?;

        let (found, slot) = self.pool.page(frame).search_slot(key)?;
        let record = if found {
            Some(self.pool.page(frame).leaf_record_id(slot)?)
        } else {
            None
        };

        self.pool.unpin(frame)?;
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const FRAMES: usize = 8;

    // the TempDir has to outlive the tree, or the file is deleted underneath it
    fn temp_db() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tree.db");
        (dir, path)
    }

    #[test]
    fn a_fresh_file_gets_a_meta_page_and_an_empty_root_leaf() {
        let (_dir, path) = temp_db();
        let mut tree = BTree::open(&path, FRAMES).unwrap();

        // meta is page 0, so the first page the tree can use is 1
        assert_eq!(tree.root().unwrap(), PageId(1));

        let frame = tree.pool.fetch_page(PageId(1)).unwrap();
        let root = tree.pool.page(frame);
        assert!(root.is_leaf());
        assert_eq!(root.slot_count(), 0);
        assert_eq!(root.next_leaf(), None);
        tree.pool.unpin(frame).unwrap();
    }

    #[test]
    fn opening_a_fresh_file_writes_both_pages_to_disk() {
        // open flushes, so the two pages must be on disk before it returns
        let (_dir, path) = temp_db();
        {
            BTree::open(&path, FRAMES).unwrap();
        }

        let len = std::fs::metadata(&path).unwrap().len();
        assert_eq!(len, 2 * crate::page::PAGE_SIZE as u64);
    }

    #[test]
    fn reopening_finds_the_same_root() {
        let (_dir, path) = temp_db();
        {
            let mut tree = BTree::open(&path, FRAMES).unwrap();
            assert_eq!(tree.root().unwrap(), PageId(1));
        }

        // the second open must take the existing path, not bootstrap again
        let mut tree = BTree::open(&path, FRAMES).unwrap();
        assert_eq!(tree.root().unwrap(), PageId(1));
        assert_eq!(tree.pool.page_count().unwrap(), 2);
    }

    #[test]
    fn a_new_root_survives_a_reopen() {
        // what a root split does: point the meta page somewhere else
        let (_dir, path) = temp_db();
        {
            let mut tree = BTree::open(&path, FRAMES).unwrap();
            tree.set_root(PageId(7)).unwrap();
            tree.pool.flush_all().unwrap();
        }

        let mut tree = BTree::open(&path, FRAMES).unwrap();
        assert_eq!(tree.root().unwrap(), PageId(7));
    }

    #[test]
    fn a_file_that_is_not_a_database_is_rejected() {
        let (_dir, path) = temp_db();
        // one page of junk, so page_count is not 0 and the magic check runs
        std::fs::write(&path, vec![0xABu8; crate::page::PAGE_SIZE]).unwrap();

        assert!(matches!(
            BTree::open(&path, FRAMES),
            Err(Error::BadMetaPage)
        ));
    }

    #[test]
    fn open_leaves_no_frame_pinned() {
        // a pin leak would only show up much later, as a full pool
        let (_dir, path) = temp_db();
        let mut tree = BTree::open(&path, 2).unwrap();

        // both frames hold open's pages, so allocating two more forces an
        // eviction each, which is only possible if open unpinned them
        for _ in 0..2 {
            let (_, frame) = tree.pool.new_page().unwrap();
            tree.pool.unpin(frame).unwrap();
        }
    }
}
