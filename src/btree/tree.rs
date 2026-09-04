#![allow(dead_code)]

use std::path::Path;

use crate::btree::node::{
    encode_internal_entry, encode_leaf_entry, internal_entry_child, internal_entry_key,
    merged_fits, set_entry_child, split_point,
};
use crate::buffer::{BufferPool, FrameId};
use crate::error::{Error, Result};
use crate::page::slotted::SlotId;
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

    pub fn delete(&mut self, key: &[u8]) -> Result<bool> {
        let root = self.root()?;
        let (removed, _) = self.delete_from(root, key)?;

        let frame = self.pool.fetch_page(root)?;
        let collapse = !self.pool.page(frame).is_leaf() && self.pool.page(frame).slot_count() == 0;
        let survivor = if collapse {
            self.pool.page(frame).rightmost_child()
        } else {
            None
        };
        self.pool.unpin(frame)?;

        if let Some(child) = survivor {
            self.set_root(child)?;
        }

        Ok(removed)
    }

    fn delete_from(&mut self, page_id: PageId, key: &[u8]) -> Result<(bool, bool)> {
        let frame = self.pool.fetch_page(page_id)?;

        if !self.pool.page(frame).is_leaf() {
            let child_slot = self.pool.page(frame).child_slot_for_key(key)?;
            let child = self.pool.page(frame).child_at(child_slot)?;
            self.pool.unpin(frame)?;

            let (removed, underfull) = self.delete_from(child, key)?;

            if !underfull {
                return Ok((removed, false));
            }

            self.rebalance_child(page_id, child_slot)?;

            let frame = self.pool.fetch_page(page_id)?;
            let underfull = self.pool.page(frame).is_underfull();
            self.pool.unpin(frame)?;

            return Ok((removed, underfull));
        }

        let (found, slot) = self.pool.page(frame).search_slot(key)?;

        if found {
            self.pool.page_for_write(frame).remove_slot_at(slot)?;
        }

        let underfull = self.pool.page(frame).is_underfull();
        self.pool.unpin(frame)?;

        Ok((found, underfull))
    }

    fn merge_fits(&mut self, parent: PageId, left_slot: SlotId) -> Result<bool> {
        let frame = self.pool.fetch_page(parent)?;
        let left = self.pool.page(frame).child_at(left_slot)?;
        let right = self.pool.page(frame).child_at(left_slot + 1)?;
        let separator = self.pool.page(frame).internal_key(left_slot)?.to_vec();
        self.pool.unpin(frame)?;

        let frame = self.pool.fetch_page(left)?;
        let is_leaf = self.pool.page(frame).is_leaf();
        let available = self.pool.page(frame).free_space() + self.pool.page(frame).frag_space();
        self.pool.unpin(frame)?;

        let frame = self.pool.fetch_page(right)?;
        let incoming = self.pool.page(frame).live_bytes();
        self.pool.unpin(frame)?;

        let separator = if is_leaf { None } else { Some(&separator[..]) };
        Ok(merged_fits(incoming, available, separator))
    }

    fn rebalance_child(&mut self, parent: PageId, child_slot: SlotId) -> Result<()> {
        let frame = self.pool.fetch_page(parent)?;
        let slot_count = self.pool.page(frame).slot_count();
        self.pool.unpin(frame)?;

        let mut pairs = Vec::new();
        if child_slot < slot_count {
            pairs.push(child_slot);
        }
        if child_slot > 0 {
            pairs.push(child_slot - 1);
        }

        for left_slot in pairs {
            if self.merge_fits(parent, left_slot)? {
                return self.merge_children(parent, left_slot);
            }
        }

        Ok(())
    }

    fn merge_children(&mut self, parent: PageId, left_slot: SlotId) -> Result<()> {
        let frame = self.pool.fetch_page(parent)?;
        let left = self.pool.page(frame).child_at(left_slot)?;
        let right = self.pool.page(frame).child_at(left_slot + 1)?;
        let separator = self.pool.page(frame).internal_key(left_slot)?.to_vec();
        self.pool.unpin(frame)?;

        let frame = self.pool.fetch_page(right)?;
        let is_leaf = self.pool.page(frame).is_leaf();
        let moved = self.pool.page(frame).entries()?;
        let right_link = self.pool.page(frame).next_leaf();
        self.pool.unpin(frame)?;

        let frame = self.pool.fetch_page(left)?;
        self.pool.page_for_write(frame).compact();
        if !is_leaf {
            let left_link = self
                .pool
                .page(frame)
                .rightmost_child()
                .expect("internal node without a rightmost child");
            let pulled_down = encode_internal_entry(left_link, &separator);
            self.pool.page_for_write(frame).append_slot(&pulled_down)?;
        }
        for entry in &moved {
            self.pool.page_for_write(frame).append_slot(entry)?;
        }
        if is_leaf {
            self.pool.page_for_write(frame).set_next_leaf(right_link);
        } else {
            self.pool
                .page_for_write(frame)
                .set_rightmost_child(right_link.expect("internal node without a rightmost child"));
        }
        self.pool.unpin(frame)?;

        let frame = self.pool.fetch_page(parent)?;
        self.pool.page_for_write(frame).remove_slot_at(left_slot)?;
        if left_slot == self.pool.page(frame).slot_count() {
            self.pool.page_for_write(frame).set_rightmost_child(left);
        } else {
            self.pool
                .page_for_write(frame)
                .set_internal_child(left_slot, left)?;
        }
        self.pool.unpin(frame)?;

        Ok(())
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

    // every pair from start up to but not including end
    pub fn scan(&mut self, start: &[u8], end: &[u8]) -> Result<Vec<(Vec<u8>, RecordId)>> {
        let mut frame = self.find_leaf(start)?;
        let (_, mut slot) = self.pool.page(frame).search_slot(start)?;
        let mut out = Vec::new();

        loop {
            while slot < self.pool.page(frame).slot_count() {
                if self.pool.page(frame).leaf_key(slot)? >= end {
                    self.pool.unpin(frame)?;
                    return Ok(out);
                }

                let key = self.pool.page(frame).leaf_key(slot)?.to_vec();
                let rid = self.pool.page(frame).leaf_record_id(slot)?;
                out.push((key, rid));
                slot += 1;
            }
            let next = self.pool.page(frame).next_leaf();
            self.pool.unpin(frame)?;

            match next {
                Some(page_id) => {
                    frame = self.pool.fetch_page(page_id)?;
                    slot = 0;
                }
                None => break,
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;
