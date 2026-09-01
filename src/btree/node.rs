#![allow(dead_code)]

use crate::error::Result;
use crate::page::{Page, PageId, slotted::SlotId};

const PAGE_TYPE_LEAF: u8 = 2;
const PAGE_TYPE_INTERNAL: u8 = 3;

const KEY_LEN_SIZE: usize = 2;
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
        let entry = self.get_record(slot)?;
        let key_len = leaf_key_len(entry);

        Ok(&entry[KEY_LEN_SIZE..KEY_LEN_SIZE + key_len])
    }

    pub(crate) fn leaf_value(&self, slot: SlotId) -> Result<&[u8]> {
        let entry = self.get_record(slot)?;
        let key_len = leaf_key_len(entry);

        Ok(&entry[KEY_LEN_SIZE + key_len..])
    }
    pub(crate) fn internal_key(&self, slot: SlotId) -> Result<&[u8]> {
        let entry = self.get_record(slot)?;
        Ok(&entry[CHILD_SIZE..])
    }
    pub(crate) fn internal_child(&self, slot: SlotId) -> Result<PageId> {
        let entry = self.get_record(slot)?;
        Ok(PageId(u64::from_le_bytes(
            entry[..CHILD_SIZE].try_into().unwrap(),
        )))
    }
}

// converting key+value as one blob to enter into a slotted page
fn encode_leaf_entry(key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut entry: Vec<u8> = Vec::with_capacity(KEY_LEN_SIZE + key.len() + value.len());
    entry.extend_from_slice(&(key.len() as u16).to_le_bytes());
    entry.extend_from_slice(key);
    entry.extend_from_slice(value);

    entry
}

fn leaf_key_len(entry: &[u8]) -> usize {
    u16::from_le_bytes(entry[..KEY_LEN_SIZE].try_into().unwrap()) as usize
}

// converting child+key as one blob to enter into a slotted page
fn encode_internal_entry(child: PageId, key: &[u8]) -> Vec<u8> {
    let mut entry: Vec<u8> = Vec::with_capacity(CHILD_SIZE + key.len());
    entry.extend_from_slice(&child.0.to_le_bytes());
    entry.extend_from_slice(key);

    entry
}
