#![allow(dead_code)]

use std::path::Path;

use crate::btree::node::encode_leaf_entry;
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
        let root = self.pool.page(frame).root_page_id();
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
                return Ok(frame); // still pinned, on purpose
            }
            let child = self.pool.page(frame).child_for_key(key)?;
            self.pool.unpin(frame)?;
            page_id = child;
        }
    }

    pub fn insert(&mut self, key: &[u8], record: RecordId) -> Result<()> {
        let frame = self.find_leaf(key)?;

        let (found, slot) = self.pool.page(frame).search_slot(key)?;

        let result = if found {
            self.pool.page_mut(frame).set_leaf_record_id(slot, record)
        } else {
            let entry = encode_leaf_entry(record, key);
            self.pool.page_mut(frame).insert_record_at(slot, &entry)
        };
        self.pool.unpin(frame)?;
        result
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
#[path = "tree_tests.rs"]
mod tests;
