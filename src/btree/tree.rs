#![allow(dead_code)]

use std::path::Path;

use crate::btree::node::{
    encode_internal_entry, encode_leaf_entry, internal_entry_child, internal_entry_key,
    set_entry_child, split_point,
};
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

            pool.page_for_write(root_frame).init_leaf();
            pool.page_for_write(meta_frame).init_meta(root_id);

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
        self.pool.page_for_write(frame).set_root_page_id(root);
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
        let root = self.root()?;

        if let Some((separator, right)) = self.insert_into(root, key, record)? {
            let (new_root_id, new_root_frame) = self.pool.new_page()?;
            self.pool
                .page_for_write(new_root_frame)
                .init_internal(right);
            let entry = encode_internal_entry(root, &separator);
            self.pool
                .page_for_write(new_root_frame)
                .append_slot(&entry)?;
            self.pool.unpin(new_root_frame)?;
            self.set_root(new_root_id)?;
        }

        Ok(())
    }

    fn insert_into(
        &mut self,
        page_id: PageId,
        key: &[u8],
        record: RecordId,
    ) -> Result<Option<(Vec<u8>, PageId)>> {
        let frame = self.pool.fetch_page(page_id)?;

        if !self.pool.page(frame).is_leaf() {
            let child = self.pool.page(frame).child_for_key(key)?;
            self.pool.unpin(frame)?;

            let Some((separator, right)) = self.insert_into(child, key, record)? else {
                return Ok(None);
            };

            let frame = self.pool.fetch_page(page_id)?;

            let (_, pos) = self.pool.page(frame).search_slot(&separator)?;
            let was_rightmost = pos == self.pool.page(frame).slot_count();

            let entry = encode_internal_entry(child, &separator);
            let attempt = self.pool.page_for_write(frame).insert_slot_at(pos, &entry);

            return match attempt {
                Ok(()) => {
                    if was_rightmost {
                        self.pool.page_for_write(frame).set_rightmost_child(right);
                    } else {
                        self.pool
                            .page_for_write(frame)
                            .set_internal_child(pos + 1, right)?;
                    }
                    self.pool.unpin(frame)?;
                    Ok(None)
                }
                Err(Error::PageFull { .. }) => {
                    let old_link = self
                        .pool
                        .page(frame)
                        .rightmost_child()
                        .expect("internal node without a rightmost child");

                    let mut entries = self.pool.page(frame).entries()?;
                    entries.insert(pos as usize, entry);

                    let link = if was_rightmost {
                        right
                    } else {
                        set_entry_child(&mut entries[pos as usize + 1], right);
                        old_link
                    };

                    let mid = split_point(&entries);
                    let promoted_child = internal_entry_child(&entries[mid]);
                    let promoted_key = internal_entry_key(&entries[mid]).to_vec();

                    let (right_id, right_frame) = self.pool.new_page()?;
                    self.pool.page_for_write(right_frame).init_internal(link);
                    for entry in &entries[mid + 1..] {
                        self.pool.page_for_write(right_frame).append_slot(entry)?;
                    }

                    self.pool
                        .page_for_write(frame)
                        .init_internal(promoted_child);
                    for entry in &entries[..mid] {
                        self.pool.page_for_write(frame).append_slot(entry)?;
                    }

                    self.pool.unpin(right_frame)?;
                    self.pool.unpin(frame)?;

                    Ok(Some((promoted_key, right_id)))
                }
                Err(e) => {
                    self.pool.unpin(frame)?;
                    Err(e)
                }
            };
        }

        let (found, slot) = self.pool.page(frame).search_slot(key)?;

        if found {
            let result = self
                .pool
                .page_for_write(frame)
                .set_leaf_record_id(slot, record);
            self.pool.unpin(frame)?;
            return result.map(|()| None);
        }

        let entry = encode_leaf_entry(record, key);
        let attempt = self.pool.page_for_write(frame).insert_slot_at(slot, &entry);

        match attempt {
            Ok(()) => {
                self.pool.unpin(frame)?;
                Ok(None)
            }
            Err(Error::PageFull { .. }) => {
                let old_next = self.pool.page(frame).next_leaf();
                let mut entries = self.pool.page(frame).entries()?;
                entries.insert(slot as usize, entry);
                let mid = split_point(&entries);

                let (right_id, right_frame) = self.pool.new_page()?;
                self.pool.page_for_write(right_frame).init_leaf();

                for entry in &entries[mid..] {
                    self.pool.page_for_write(right_frame).append_slot(entry)?;
                }
                self.pool.page_for_write(frame).init_leaf();

                for entry in &entries[..mid] {
                    self.pool.page_for_write(frame).append_slot(entry)?;
                }

                self.pool
                    .page_for_write(right_frame)
                    .set_next_leaf(old_next);
                self.pool
                    .page_for_write(frame)
                    .set_next_leaf(Some(right_id));

                let separator = self.pool.page(right_frame).leaf_key(0)?.to_vec();

                self.pool.unpin(right_frame)?;
                self.pool.unpin(frame)?;

                Ok(Some((separator, right_id)))
            }
            Err(e) => {
                self.pool.unpin(frame)?;
                Err(e)
            }
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
#[path = "tree_tests.rs"]
mod tests;
