// TODO: drop once the buffer pool and B+tree call into this layer.
#![allow(dead_code)]

use crate::error::{Error, Result};
use crate::page::{PAGE_SIZE, Page};

pub(crate) type SlotId = u16;

const OFF_LSN: usize = 0; // u64 - log sequence number (WAL)
const OFF_CHECKSUM: usize = 8; // u32 - CRC32 of the rest of the page
const OFF_PAGE_TYPE: usize = 12; // u8  - slotted / leaf / internal
const OFF_FLAGS: usize = 13; // u8  - reserved
const OFF_SLOT_COUNT: usize = 14; // u16 - number of slot entries
const OFF_FREE_PTR: usize = 16; // u16 - start of the data region
const OFF_FREE_BYTES: usize = 18; // u16 - free bytes, excluding fragmentation
const OFF_FRAG_BYTES: usize = 20; // u16 - bytes stranded by removals, freed by compaction
const OFF_RESERVED: usize = 22; // 2 bytes spare
const OFF_LINK: usize = 24; // 8 bytes for link(PageID)
pub(crate) const HEADER_SIZE: usize = 32;

const SLOT_SIZE: usize = 4; // offset (2 bytes) + length (2 bytes)

const PAGE_TYPE_SLOTTED: u8 = 1;

impl Page {
    // A zeroed page is not a valid slotted page. Call this once on allocation.
    pub(crate) fn init_slotted(&mut self) {
        self.write_u64(OFF_LSN, 0);
        self.write_u32(OFF_CHECKSUM, 0);
        self.write_u8(OFF_PAGE_TYPE, PAGE_TYPE_SLOTTED);
        self.write_u8(OFF_FLAGS, 0);
        self.write_u16(OFF_SLOT_COUNT, 0);
        // data grows down from the end, so the free pointer starts past the last byte
        self.write_u16(OFF_FREE_PTR, PAGE_SIZE as u16);
        self.write_u16(OFF_FREE_BYTES, (PAGE_SIZE - HEADER_SIZE) as u16);
        self.write_u16(OFF_FRAG_BYTES, 0);
        self.write_u16(OFF_RESERVED, 0);
        self.write_u64(OFF_LINK, 0);
    }

    pub(crate) fn slot_count(&self) -> u16 {
        self.read_u16(OFF_SLOT_COUNT)
    }

    pub(crate) fn free_space(&self) -> usize {
        self.read_u16(OFF_FREE_BYTES) as usize
    }

    // bytes a compaction would hand back, on top of free_space
    pub(crate) fn frag_space(&self) -> usize {
        self.read_u16(OFF_FRAG_BYTES) as usize
    }

    pub(crate) fn page_type(&self) -> u8 {
        self.read_u8(OFF_PAGE_TYPE)
    }

    pub(crate) fn set_page_type(&mut self, page_type: u8) {
        self.write_u8(OFF_PAGE_TYPE, page_type);
    }

    pub(crate) fn link(&self) -> u64 {
        self.read_u64(OFF_LINK)
    }

    pub(crate) fn set_link(&mut self, value: u64) {
        self.write_u64(OFF_LINK, value);
    }

    pub(crate) fn insert_record(&mut self, record: &[u8]) -> Result<SlotId> {
        if self.free_space() < SLOT_SIZE + record.len() {
            return Err(Error::PageFull {
                size: record.len() + SLOT_SIZE,
                free: self.free_space(),
            });
        }

        // ids are 0-based, so with N slots the next free id is N
        let new_slot = self.slot_count();
        let record_offset = self.read_u16(OFF_FREE_PTR) as usize - record.len();
        let free_left = self.free_space() - record.len() - SLOT_SIZE;

        self.write_bytes(record_offset, record);
        self.write_slot(new_slot, record_offset as u16, record.len() as u16);

        // header last, so an early return can never leave a half-updated page
        self.write_u16(OFF_FREE_PTR, record_offset as u16);
        self.write_u16(OFF_FREE_BYTES, free_left as u16);
        self.write_u16(OFF_SLOT_COUNT, new_slot + 1);

        Ok(new_slot)
    }

    pub(crate) fn insert_record_at(&mut self, slot: SlotId, record: &[u8]) -> Result<()> {
        if slot > self.slot_count() {
            return Err(Error::NoSuchSlot(slot));
        }

        if self.free_space() < SLOT_SIZE + record.len() {
            return Err(Error::PageFull {
                size: record.len() + SLOT_SIZE,
                free: self.free_space(),
            });
        }
        for i in (slot..self.slot_count()).rev() {
            let (offset, len) = self.read_slot(i);
            self.write_slot(i + 1, offset, len);
        }

        let record_offset = self.read_u16(OFF_FREE_PTR) as usize - record.len();
        let free_left = self.free_space() - record.len() - SLOT_SIZE;

        self.write_bytes(record_offset, record);
        self.write_slot(slot, record_offset as u16, record.len() as u16);

        // header updation
        self.write_u16(OFF_FREE_PTR, record_offset as u16);
        self.write_u16(OFF_FREE_BYTES, free_left as u16);
        self.write_u16(OFF_SLOT_COUNT, self.slot_count() + 1);

        Ok(())
    }

    pub(crate) fn remove_record_at(&mut self, slot: SlotId) -> Result<()> {
        // can't remove a thing which does not exist
        if slot >= self.slot_count() {
            return Err(Error::NoSuchSlot(slot));
        }
        // read the size of record
        let (_, len) = self.read_slot(slot);

        // shift records by 1 place left
        for i in slot + 1..self.slot_count() {
            let (offset, len) = self.read_slot(i);
            self.write_slot(i - 1, offset, len);
        }

        self.write_u16(OFF_FREE_BYTES, (self.free_space() + SLOT_SIZE) as u16);
        self.write_u16(OFF_FRAG_BYTES, (self.frag_space() + len as usize) as u16);
        self.write_u16(OFF_SLOT_COUNT, self.slot_count() - 1);

        Ok(())
    }

    pub(crate) fn get_record(&self, slot: SlotId) -> Result<&[u8]> {
        if slot >= self.slot_count() {
            return Err(Error::NoSuchSlot(slot));
        }

        let (offset, len) = self.read_slot(slot);
        // offset 0 lands inside the header, so it can never be a live record
        if offset == 0 {
            return Err(Error::SlotDeleted(slot));
        }

        Ok(self.read_bytes(offset as usize, len as usize))
    }

    pub(crate) fn get_record_mut(&mut self, slot: SlotId) -> Result<&mut [u8]> {
        if slot >= self.slot_count() {
            return Err(Error::NoSuchSlot(slot));
        }

        let (offset, len) = self.read_slot(slot);
        if offset == 0 {
            return Err(Error::SlotDeleted(slot));
        }

        Ok(self.bytes_mut(offset as usize, len as usize))
    }

    pub(crate) fn delete_record(&mut self, slot: SlotId) -> Result<()> {
        if slot >= self.slot_count() {
            return Err(Error::NoSuchSlot(slot));
        }

        let (offset, _) = self.read_slot(slot);
        if offset == 0 {
            return Err(Error::SlotDeleted(slot));
        }

        // tombstone only: the record bytes stay put until compaction reclaims them
        self.write_slot(slot, 0, 0);
        Ok(())
    }

    fn slot_entry_pos(slot: SlotId) -> usize {
        HEADER_SIZE + slot as usize * SLOT_SIZE
    }

    fn read_slot(&self, slot: SlotId) -> (u16, u16) {
        let pos = Self::slot_entry_pos(slot);
        (self.read_u16(pos), self.read_u16(pos + 2))
    }

    fn write_slot(&mut self, slot: SlotId, offset: u16, len: u16) {
        let pos = Self::slot_entry_pos(slot);
        self.write_u16(pos, offset);
        self.write_u16(pos + 2, len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slotted_page() -> Page {
        let mut page = Page::new();
        page.init_slotted();
        page
    }

    // free space recomputed from the layout, ignoring the cached header field
    fn derived_free_space(page: &Page) -> usize {
        page.read_u16(OFF_FREE_PTR) as usize
            - (HEADER_SIZE + page.slot_count() as usize * SLOT_SIZE)
    }

    #[test]
    fn init_gives_an_empty_page() {
        let page = slotted_page();

        assert_eq!(page.slot_count(), 0);
        assert_eq!(page.free_space(), PAGE_SIZE - HEADER_SIZE);
        assert_eq!(page.read_u8(OFF_PAGE_TYPE), PAGE_TYPE_SLOTTED);
        assert_eq!(page.read_u16(OFF_FREE_PTR) as usize, PAGE_SIZE);
    }

    #[test]
    fn an_uninitialised_page_accepts_nothing() {
        // allocate_page hands back zeros, so init_slotted is the caller's job
        let mut page = Page::new();

        assert!(matches!(
            page.insert_record(b"x"),
            Err(Error::PageFull { .. })
        ));
    }

    #[test]
    fn three_records_round_trip() {
        let mut page = slotted_page();

        assert_eq!(page.insert_record(b"first").unwrap(), 0);
        assert_eq!(page.insert_record(b"second one").unwrap(), 1);
        assert_eq!(page.insert_record(b"third record").unwrap(), 2);

        assert_eq!(page.get_record(0).unwrap(), b"first");
        assert_eq!(page.get_record(1).unwrap(), b"second one");
        assert_eq!(page.get_record(2).unwrap(), b"third record");
        assert_eq!(page.slot_count(), 3);
    }

    #[test]
    fn records_land_at_descending_offsets() {
        // a constant offset would pass every round trip test while each record
        // quietly overwrote the last, so check the offsets themselves
        let mut page = slotted_page();
        for _ in 0..3 {
            page.insert_record(b"same bytes").unwrap();
        }

        let (first, _) = page.read_slot(0);
        let (second, _) = page.read_slot(1);
        let (third, _) = page.read_slot(2);

        assert!(
            first > second && second > third,
            "data grows down, got {first} {second} {third}"
        );
    }

    #[test]
    fn deleting_the_middle_leaves_the_others_readable() {
        let mut page = slotted_page();
        page.insert_record(b"keep me").unwrap();
        page.insert_record(b"delete me").unwrap();
        page.insert_record(b"keep me too").unwrap();

        page.delete_record(1).unwrap();

        assert_eq!(page.get_record(0).unwrap(), b"keep me");
        assert!(matches!(page.get_record(1), Err(Error::SlotDeleted(1))));
        assert_eq!(page.get_record(2).unwrap(), b"keep me too");
        // the slot array never shrinks, dead entries still count
        assert_eq!(page.slot_count(), 3);
    }

    #[test]
    fn insert_after_delete_takes_a_fresh_slot() {
        let mut page = slotted_page();
        page.insert_record(b"zero").unwrap();
        page.insert_record(b"one").unwrap();
        page.insert_record(b"two").unwrap();
        page.delete_record(1).unwrap();

        // no slot reuse yet, so the next id is 3 and slot 1 stays dead
        assert_eq!(page.insert_record(b"three").unwrap(), 3);

        assert_eq!(page.get_record(0).unwrap(), b"zero");
        assert!(matches!(page.get_record(1), Err(Error::SlotDeleted(1))));
        assert_eq!(page.get_record(2).unwrap(), b"two");
        assert_eq!(page.get_record(3).unwrap(), b"three");
    }

    #[test]
    fn deleting_twice_is_an_error() {
        let mut page = slotted_page();
        page.insert_record(b"gone").unwrap();

        assert!(page.delete_record(0).is_ok());
        assert!(matches!(page.delete_record(0), Err(Error::SlotDeleted(0))));
    }

    #[test]
    fn slots_that_were_never_handed_out_are_an_error() {
        let mut page = slotted_page();
        assert!(matches!(page.get_record(0), Err(Error::NoSuchSlot(0))));

        page.insert_record(b"only one").unwrap();
        assert!(matches!(page.get_record(1), Err(Error::NoSuchSlot(1))));
        assert!(matches!(page.delete_record(9), Err(Error::NoSuchSlot(9))));
    }

    #[test]
    fn a_record_can_fill_the_page_exactly() {
        let mut page = slotted_page();
        let biggest = page.free_space() - SLOT_SIZE;

        page.insert_record(&vec![7u8; biggest]).unwrap();

        assert_eq!(page.free_space(), 0);
        assert_eq!(page.get_record(0).unwrap().len(), biggest);
        assert!(matches!(
            page.insert_record(b"x"),
            Err(Error::PageFull { .. })
        ));
    }

    #[test]
    fn inserting_until_full_leaves_every_record_intact() {
        let mut page = slotted_page();
        let mut ids = Vec::new();

        loop {
            let record = vec![ids.len() as u8; 100];
            match page.insert_record(&record) {
                Ok(slot) => ids.push(slot),
                Err(Error::PageFull { .. }) => break,
                Err(e) => panic!("unexpected error: {e}"),
            }
        }

        assert!(ids.len() > 30, "expected a full page, got {}", ids.len());
        for (i, &slot) in ids.iter().enumerate() {
            assert_eq!(page.get_record(slot).unwrap(), &vec![i as u8; 100][..]);
        }
    }

    #[test]
    fn stored_free_space_matches_the_derived_value() {
        // the price of caching free space in the header is that it can drift
        let mut page = slotted_page();
        assert_eq!(page.free_space(), derived_free_space(&page));

        for i in 0..5usize {
            page.insert_record(&vec![i as u8; 40 + i]).unwrap();
            assert_eq!(page.free_space(), derived_free_space(&page));
        }

        page.delete_record(2).unwrap();
        assert_eq!(page.free_space(), derived_free_space(&page));
    }

    // the records in slot order, so a test can state the whole page in one line
    fn records(page: &Page) -> Vec<Vec<u8>> {
        (0..page.slot_count())
            .map(|slot| page.get_record(slot).unwrap().to_vec())
            .collect()
    }

    fn page_with(records: &[&[u8]]) -> Page {
        let mut page = slotted_page();
        for record in records {
            page.insert_record(record).unwrap();
        }
        page
    }

    #[test]
    fn inserting_at_the_front_pushes_everything_right() {
        let mut page = page_with(&[b"b", b"c"]);

        page.insert_record_at(0, b"a").unwrap();

        assert_eq!(records(&page), vec![b"a", b"b", b"c"]);
        assert_eq!(page.slot_count(), 3);
    }

    #[test]
    fn inserting_in_the_middle_shifts_only_the_tail() {
        let mut page = page_with(&[b"a", b"c", b"d"]);

        page.insert_record_at(1, b"b").unwrap();

        assert_eq!(records(&page), vec![b"a", b"b", b"c", b"d"]);
    }

    #[test]
    fn inserting_at_slot_count_appends() {
        // this is the position search_slot returns for a key past the last one
        let mut page = page_with(&[b"a", b"b"]);

        page.insert_record_at(2, b"c").unwrap();

        assert_eq!(records(&page), vec![b"a", b"b", b"c"]);
    }

    #[test]
    fn inserting_past_the_end_is_an_error() {
        let mut page = page_with(&[b"a", b"b"]);

        assert!(matches!(
            page.insert_record_at(3, b"x"),
            Err(Error::NoSuchSlot(3))
        ));
        // and the page is untouched
        assert_eq!(records(&page), vec![b"a", b"b"]);
        assert_eq!(page.slot_count(), 2);
    }

    #[test]
    fn shifting_moves_slot_entries_not_record_bytes() {
        // the whole point of the design: reordering is a slot array edit
        let mut page = page_with(&[b"b", b"c"]);
        let (b_offset, _) = page.read_slot(0);
        let (c_offset, _) = page.read_slot(1);

        page.insert_record_at(0, b"a").unwrap();

        assert_eq!(page.read_slot(1).0, b_offset);
        assert_eq!(page.read_slot(2).0, c_offset);
    }

    #[test]
    fn a_run_of_sorted_inserts_keeps_the_page_ordered() {
        // insert in a scrambled order, each one at the position a search would
        // hand back, and the page must come out sorted
        let mut page = slotted_page();
        for record in [
            b"m".as_slice(),
            b"c",
            b"t",
            b"a",
            b"z",
            b"f",
            b"q",
            b"b",
            b"y",
        ] {
            let at = records(&page).partition_point(|existing| existing.as_slice() < record);
            page.insert_record_at(at as SlotId, record).unwrap();
        }

        assert_eq!(
            records(&page),
            vec![b"a", b"b", b"c", b"f", b"m", b"q", b"t", b"y", b"z"]
        );
    }

    #[test]
    fn a_full_page_rejects_a_positional_insert() {
        let mut page = slotted_page();
        let biggest = page.free_space() - SLOT_SIZE;
        page.insert_record(&vec![7u8; biggest]).unwrap();

        assert!(matches!(
            page.insert_record_at(0, b"x"),
            Err(Error::PageFull { .. })
        ));
        assert_eq!(page.slot_count(), 1);
    }

    #[test]
    fn free_space_stays_consistent_after_positional_inserts() {
        let mut page = slotted_page();

        for i in 0..6usize {
            page.insert_record_at(0, &vec![i as u8; 30 + i]).unwrap();
            assert_eq!(page.free_space(), derived_free_space(&page));
        }
    }

    #[test]
    fn removing_the_front_pulls_everything_left() {
        let mut page = page_with(&[b"a", b"b", b"c"]);

        page.remove_record_at(0).unwrap();

        assert_eq!(records(&page), vec![b"b", b"c"]);
        assert_eq!(page.slot_count(), 2);
    }

    #[test]
    fn removing_the_middle_closes_the_gap() {
        // no tombstone, unlike delete_record: the array stays dense
        let mut page = page_with(&[b"a", b"b", b"c", b"d"]);

        page.remove_record_at(1).unwrap();

        assert_eq!(records(&page), vec![b"a", b"c", b"d"]);
    }

    #[test]
    fn removing_the_last_shifts_nothing() {
        let mut page = page_with(&[b"a", b"b", b"c"]);

        page.remove_record_at(2).unwrap();

        assert_eq!(records(&page), vec![b"a", b"b"]);
    }

    #[test]
    fn removing_past_the_end_is_an_error() {
        let mut page = page_with(&[b"a", b"b"]);

        assert!(matches!(
            page.remove_record_at(2),
            Err(Error::NoSuchSlot(2))
        ));
        assert_eq!(records(&page), vec![b"a", b"b"]);
    }

    #[test]
    fn removing_strands_the_record_bytes_but_frees_the_slot() {
        // sizes differ so a wrong slot's length would show up
        let mut page = page_with(&[b"aa", b"bbbb", b"cccccc"]);
        let free_before = page.free_space();
        let free_ptr_before = page.read_u16(OFF_FREE_PTR);

        page.remove_record_at(1).unwrap();

        assert_eq!(page.frag_space(), 4);
        assert_eq!(page.free_space(), free_before + SLOT_SIZE);
        // the data region never moved, that is what makes those 4 bytes stranded
        assert_eq!(page.read_u16(OFF_FREE_PTR), free_ptr_before);
    }

    #[test]
    fn fragmentation_adds_up_over_several_removals() {
        let mut page = page_with(&[b"aa", b"bbbb", b"cccccc"]);

        page.remove_record_at(0).unwrap();
        assert_eq!(page.frag_space(), 2);
        page.remove_record_at(1).unwrap();
        assert_eq!(page.frag_space(), 8);

        assert_eq!(records(&page), vec![b"bbbb"]);
    }

    #[test]
    fn removing_every_record_leaves_an_empty_page() {
        let mut page = page_with(&[b"a", b"b", b"c"]);

        for _ in 0..3 {
            page.remove_record_at(0).unwrap();
        }

        assert_eq!(page.slot_count(), 0);
        assert!(matches!(page.get_record(0), Err(Error::NoSuchSlot(0))));
        assert!(matches!(
            page.remove_record_at(0),
            Err(Error::NoSuchSlot(0))
        ));
    }

    #[test]
    fn free_space_stays_consistent_across_removals() {
        let mut page = slotted_page();
        for i in 0..6usize {
            page.insert_record(&vec![i as u8; 30 + i]).unwrap();
        }

        while page.slot_count() > 0 {
            page.remove_record_at(0).unwrap();
            assert_eq!(page.free_space(), derived_free_space(&page));
        }
    }

    #[test]
    fn insert_after_remove_keeps_the_order() {
        // the tree does exactly this, remove a key then put another in its place
        let mut page = page_with(&[b"a", b"c", b"e"]);

        page.remove_record_at(1).unwrap();
        page.insert_record_at(1, b"b").unwrap();

        assert_eq!(records(&page), vec![b"a", b"b", b"e"]);
    }
}
