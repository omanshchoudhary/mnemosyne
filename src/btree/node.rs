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
        let entry = self.get_record(slot)?;
        Ok(&entry[RID_SIZE..])
    }

    // where the record actually lives
    pub(crate) fn leaf_record_id(&self, slot: SlotId) -> Result<RecordId> {
        let entry = self.get_record(slot)?;
        Ok(RecordId::from_bytes(&entry[..RID_SIZE]))
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
fn encode_leaf_entry(rid: RecordId, key: &[u8]) -> Vec<u8> {
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

    fn rid(page: u64, slot: SlotId) -> RecordId {
        RecordId {
            page: PageId(page),
            slot,
        }
    }

    // inserts an encoded pair and hands back the slot it landed in
    fn put_leaf(page: &mut Page, key: &[u8], record: RecordId) -> SlotId {
        page.insert_record(&encode_leaf_entry(record, key)).unwrap()
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
        let slot = put_leaf(&mut page, b"apple", rid(57, 3));

        assert_eq!(page.leaf_key(slot).unwrap(), b"apple");
        assert_eq!(page.leaf_record_id(slot).unwrap(), rid(57, 3));
    }

    #[test]
    fn a_record_id_past_a_single_byte_survives() {
        // both halves are wider than a byte, and a truncating write would
        // still pass on small ids
        let mut page = leaf_page();
        let slot = put_leaf(&mut page, b"key", rid(70_000, 300));

        assert_eq!(page.leaf_record_id(slot).unwrap(), rid(70_000, 300));
    }

    #[test]
    fn two_leaf_entries_stay_separate() {
        // one entry alone would still pass with a wrong offset or length
        let mut page = leaf_page();
        let first = put_leaf(&mut page, b"a", rid(1, 0));
        let second = put_leaf(&mut page, b"much longer key", rid(2, 9));

        assert_eq!(page.leaf_key(first).unwrap(), b"a");
        assert_eq!(page.leaf_record_id(first).unwrap(), rid(1, 0));
        assert_eq!(page.leaf_key(second).unwrap(), b"much longer key");
        assert_eq!(page.leaf_record_id(second).unwrap(), rid(2, 9));
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

    // insert_record appends, so feeding sorted keys leaves the slot array sorted
    fn leaf_with(keys: &[&[u8]]) -> Page {
        let mut page = leaf_page();
        for (i, key) in keys.iter().enumerate() {
            put_leaf(&mut page, key, rid(1, i as SlotId));
        }
        page
    }

    #[test]
    fn searching_an_empty_page_lands_at_slot_zero() {
        // the signed bounds exist for this case, slot_count - 1 is -1 here
        let page = leaf_page();

        assert_eq!(page.search_slot(b"anything").unwrap(), (false, 0));
    }

    #[test]
    fn every_key_present_is_found_at_its_own_slot() {
        let page = leaf_with(&[b"a", b"c", b"e", b"g", b"i"]);

        assert_eq!(page.search_slot(b"a").unwrap(), (true, 0));
        assert_eq!(page.search_slot(b"c").unwrap(), (true, 1));
        assert_eq!(page.search_slot(b"e").unwrap(), (true, 2));
        assert_eq!(page.search_slot(b"g").unwrap(), (true, 3));
        assert_eq!(page.search_slot(b"i").unwrap(), (true, 4));
    }

    #[test]
    fn a_miss_reports_where_the_key_belongs() {
        let page = leaf_with(&[b"a", b"c", b"e", b"g", b"i"]);

        // before everything, between neighbours, and past the end
        assert_eq!(page.search_slot(b"A").unwrap(), (false, 0));
        assert_eq!(page.search_slot(b"b").unwrap(), (false, 1));
        assert_eq!(page.search_slot(b"d").unwrap(), (false, 2));
        assert_eq!(page.search_slot(b"h").unwrap(), (false, 4));
        assert_eq!(page.search_slot(b"z").unwrap(), (false, 5));
    }

    #[test]
    fn a_single_entry_page_answers_both_sides() {
        let page = leaf_with(&[b"m"]);

        assert_eq!(page.search_slot(b"m").unwrap(), (true, 0));
        assert_eq!(page.search_slot(b"a").unwrap(), (false, 0));
        assert_eq!(page.search_slot(b"z").unwrap(), (false, 1));
    }

    #[test]
    fn a_prefix_sorts_before_the_longer_key() {
        // byte order, not length order: "ab" comes after "a"
        let page = leaf_with(&[b"a", b"ab", b"abc"]);

        assert_eq!(page.search_slot(b"ab").unwrap(), (true, 1));
        assert_eq!(page.search_slot(b"aa").unwrap(), (false, 1));
        assert_eq!(page.search_slot(b"abcd").unwrap(), (false, 3));
    }

    #[test]
    fn search_works_on_an_internal_node_too() {
        // key_at picks the accessor by page type, so the same search must serve both
        let mut page = internal_page(PageId(99));
        put_internal(&mut page, PageId(1), b"d");
        put_internal(&mut page, PageId(2), b"m");
        put_internal(&mut page, PageId(3), b"t");

        assert_eq!(page.search_slot(b"m").unwrap(), (true, 1));
        assert_eq!(page.search_slot(b"a").unwrap(), (false, 0));
        assert_eq!(page.search_slot(b"z").unwrap(), (false, 3));
    }

    #[test]
    fn every_probe_is_correct_on_a_bigger_page() {
        // 5 entries only ever exercise a couple of loop shapes
        let keys: Vec<Vec<u8>> = (0..60u32).map(|i| format!("{i:04}").into_bytes()).collect();
        let mut page = leaf_page();
        for (i, key) in keys.iter().enumerate() {
            put_leaf(&mut page, key, rid(1, i as SlotId));
        }

        for (i, key) in keys.iter().enumerate() {
            assert_eq!(page.search_slot(key).unwrap(), (true, i as SlotId));
        }
        assert_eq!(page.search_slot(b"0000x").unwrap(), (false, 1));
        assert_eq!(page.search_slot(b"9999").unwrap(), (false, 60));
    }
}
