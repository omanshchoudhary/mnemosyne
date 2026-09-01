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

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf_page() -> Page {
        let mut page = Page::new();
        page.init_leaf();
        page
    }

    fn internal_page(rightmost: PageId) -> Page {
        let mut page = Page::new();
        page.init_internal(rightmost);
        page
    }

    // inserts an encoded pair and hands back the slot it landed in
    fn put_leaf(page: &mut Page, key: &[u8], value: &[u8]) -> SlotId {
        page.insert_record(&encode_leaf_entry(key, value)).unwrap()
    }

    fn put_internal(page: &mut Page, child: PageId, key: &[u8]) -> SlotId {
        page.insert_record(&encode_internal_entry(child, key))
            .unwrap()
    }

    #[test]
    fn init_leaf_marks_the_page_a_leaf() {
        let page = leaf_page();

        assert!(page.is_leaf());
        assert!(!page.is_internal());
        // a fresh leaf is the last one in the chain until it splits
        assert_eq!(page.next_leaf(), None);
        assert_eq!(page.slot_count(), 0);
    }

    #[test]
    fn init_internal_sets_the_rightmost_child() {
        let page = internal_page(PageId(7));

        assert!(page.is_internal());
        assert!(!page.is_leaf());
        assert_eq!(page.rightmost_child(), Some(PageId(7)));
        assert_eq!(page.slot_count(), 0);
    }

    #[test]
    fn the_next_leaf_pointer_round_trips() {
        let mut page = leaf_page();

        page.set_next_leaf(Some(PageId(42)));
        assert_eq!(page.next_leaf(), Some(PageId(42)));

        page.set_next_leaf(None);
        assert_eq!(page.next_leaf(), None);
    }

    #[test]
    fn a_leaf_entry_round_trips() {
        let mut page = leaf_page();
        let slot = put_leaf(&mut page, b"apple", b"a red fruit");

        assert_eq!(page.leaf_key(slot).unwrap(), b"apple");
        assert_eq!(page.leaf_value(slot).unwrap(), b"a red fruit");
    }

    #[test]
    fn a_leaf_value_can_be_empty() {
        // the value is whatever follows the key, so a zero length one is the
        // case where the end of the slice has to be exactly right
        let mut page = leaf_page();
        let slot = put_leaf(&mut page, b"key", b"");

        assert_eq!(page.leaf_key(slot).unwrap(), b"key");
        assert_eq!(page.leaf_value(slot).unwrap(), b"");
    }

    #[test]
    fn two_leaf_entries_stay_separate() {
        // one entry alone would still pass with a wrong offset or length
        let mut page = leaf_page();
        let first = put_leaf(&mut page, b"a", b"short");
        let second = put_leaf(&mut page, b"much longer key", b"v");

        assert_eq!(page.leaf_key(first).unwrap(), b"a");
        assert_eq!(page.leaf_value(first).unwrap(), b"short");
        assert_eq!(page.leaf_key(second).unwrap(), b"much longer key");
        assert_eq!(page.leaf_value(second).unwrap(), b"v");
    }

    #[test]
    fn an_internal_entry_round_trips() {
        let mut page = internal_page(PageId(99));
        let slot = put_internal(&mut page, PageId(5), b"m");

        assert_eq!(page.internal_child(slot).unwrap(), PageId(5));
        assert_eq!(page.internal_key(slot).unwrap(), b"m");
        // separators live in slots, the n+1th child stays in the header
        assert_eq!(page.rightmost_child(), Some(PageId(99)));
    }

    #[test]
    fn two_internal_entries_stay_separate() {
        let mut page = internal_page(PageId(99));
        let first = put_internal(&mut page, PageId(1), b"d");
        let second = put_internal(&mut page, PageId(2), b"a much longer separator");

        assert_eq!(page.internal_child(first).unwrap(), PageId(1));
        assert_eq!(page.internal_key(first).unwrap(), b"d");
        assert_eq!(page.internal_child(second).unwrap(), PageId(2));
        assert_eq!(
            page.internal_key(second).unwrap(),
            b"a much longer separator"
        );
    }
}
