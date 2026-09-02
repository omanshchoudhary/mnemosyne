#![allow(dead_code)]

use crate::error::Result;
use crate::page::{Page, PageId, RecordId, slotted::SlotId};

const PAGE_TYPE_LEAF: u8 = 2;
const PAGE_TYPE_INTERNAL: u8 = 3;

const RID_SIZE: usize = RecordId::SIZE;
const CHILD_SIZE: usize = 8;

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

    pub(crate) fn init_leaf(&mut self) {
        self.init_slotted();
        self.set_page_type(PAGE_TYPE_LEAF);
        self.set_next_leaf(None);
    }

    pub(crate) fn init_internal(&mut self, rightmost: PageId) {
        self.init_slotted();
        self.set_page_type(PAGE_TYPE_INTERNAL);
        self.set_rightmost_child(rightmost);
    }

    pub(crate) fn leaf_key(&self, slot: SlotId) -> Result<&[u8]> {
        let entry = self.slot_bytes(slot)?;
        Ok(&entry[RID_SIZE..])
    }

    // where the record actually lives
    pub(crate) fn leaf_record_id(&self, slot: SlotId) -> Result<RecordId> {
        let entry = self.slot_bytes(slot)?;
        Ok(RecordId::from_bytes(&entry[..RID_SIZE]))
    }

    pub(crate) fn set_leaf_record_id(&mut self, slot: SlotId, rid: RecordId) -> Result<()> {
        let entry = self.slot_bytes_mut(slot)?;
        entry[..RID_SIZE].copy_from_slice(&rid.to_bytes());
        Ok(())
    }

    pub(crate) fn internal_key(&self, slot: SlotId) -> Result<&[u8]> {
        let entry = self.slot_bytes(slot)?;
        Ok(&entry[CHILD_SIZE..])
    }
    pub(crate) fn internal_child(&self, slot: SlotId) -> Result<PageId> {
        let entry = self.slot_bytes(slot)?;
        Ok(PageId(u64::from_le_bytes(
            entry[..CHILD_SIZE].try_into().unwrap(),
        )))
    }

    // returns key of a slot in a page for binary search comparisions
    pub(crate) fn key_at(&self, slot: SlotId) -> Result<&[u8]> {
        if self.is_leaf() {
            self.leaf_key(slot)
        } else {
            self.internal_key(slot)
        }
    }

    // Binary Search
    pub(crate) fn search_slot(&self, key: &[u8]) -> Result<(bool, SlotId)> {
        let mut low: i32 = 0;
        let mut high: i32 = self.slot_count() as i32 - 1;

        while low <= high {
            let mid = (low + high) / 2;
            let mid_key = self.key_at(mid as SlotId)?;

            if mid_key == key {
                return Ok((true, mid as SlotId));
            } else if mid_key > key {
                high = mid - 1;
            } else {
                low = mid + 1;
            }
        }

        Ok((false, low as SlotId))
    }

    pub(crate) fn child_for_key(&self, key: &[u8]) -> Result<PageId> {
        let (found, slot) = self.search_slot(key)?;

        let child_slot = if found { slot + 1 } else { slot };

        if child_slot == self.slot_count() {
            return Ok(self
                .rightmost_child()
                .expect("internal node without a rightmost child"));
        }

        self.internal_child(child_slot)
    }
}

// converting rid+key as one blob to enter into a slotted page
pub(crate) fn encode_leaf_entry(rid: RecordId, key: &[u8]) -> Vec<u8> {
    let mut entry: Vec<u8> = Vec::with_capacity(RID_SIZE + key.len());
    entry.extend_from_slice(&rid.to_bytes());
    entry.extend_from_slice(key);

    entry
}

// converting child+key as one blob to enter into a slotted page
fn encode_internal_entry(child: PageId, key: &[u8]) -> Vec<u8> {
    let mut entry: Vec<u8> = Vec::with_capacity(CHILD_SIZE + key.len());
    entry.extend_from_slice(&child.0.to_le_bytes());
    entry.extend_from_slice(key);

    entry
}

#[cfg(test)]
#[path = "node_tests.rs"]
mod tests;
