use super::*;

fn slotted_page() -> Page {
    let mut page = Page::new();
    page.init_slotted();
    page
}

// free space recomputed from the layout, ignoring the cached header field
fn derived_free_space(page: &Page) -> usize {
    page.read_u16(OFF_FREE_PTR) as usize - (HEADER_SIZE + page.slot_count() as usize * SLOT_SIZE)
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
        page.append_slot(b"x"),
        Err(Error::PageFull { .. })
    ));
}

#[test]
fn three_records_round_trip() {
    let mut page = slotted_page();

    assert_eq!(page.append_slot(b"first").unwrap(), 0);
    assert_eq!(page.append_slot(b"second one").unwrap(), 1);
    assert_eq!(page.append_slot(b"third record").unwrap(), 2);

    assert_eq!(page.slot_bytes(0).unwrap(), b"first");
    assert_eq!(page.slot_bytes(1).unwrap(), b"second one");
    assert_eq!(page.slot_bytes(2).unwrap(), b"third record");
    assert_eq!(page.slot_count(), 3);
}

#[test]
fn records_land_at_descending_offsets() {
    // a constant offset would pass every round trip test while each record
    // quietly overwrote the last, so check the offsets themselves
    let mut page = slotted_page();
    for _ in 0..3 {
        page.append_slot(b"same bytes").unwrap();
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
    page.append_slot(b"keep me").unwrap();
    page.append_slot(b"delete me").unwrap();
    page.append_slot(b"keep me too").unwrap();

    page.tombstone_slot(1).unwrap();

    assert_eq!(page.slot_bytes(0).unwrap(), b"keep me");
    assert!(matches!(page.slot_bytes(1), Err(Error::SlotDeleted(1))));
    assert_eq!(page.slot_bytes(2).unwrap(), b"keep me too");
    // the slot array never shrinks, dead entries still count
    assert_eq!(page.slot_count(), 3);
}

#[test]
fn insert_after_delete_takes_a_fresh_slot() {
    let mut page = slotted_page();
    page.append_slot(b"zero").unwrap();
    page.append_slot(b"one").unwrap();
    page.append_slot(b"two").unwrap();
    page.tombstone_slot(1).unwrap();

    // no slot reuse yet, so the next id is 3 and slot 1 stays dead
    assert_eq!(page.append_slot(b"three").unwrap(), 3);

    assert_eq!(page.slot_bytes(0).unwrap(), b"zero");
    assert!(matches!(page.slot_bytes(1), Err(Error::SlotDeleted(1))));
    assert_eq!(page.slot_bytes(2).unwrap(), b"two");
    assert_eq!(page.slot_bytes(3).unwrap(), b"three");
}

#[test]
fn deleting_twice_is_an_error() {
    let mut page = slotted_page();
    page.append_slot(b"gone").unwrap();

    assert!(page.tombstone_slot(0).is_ok());
    assert!(matches!(page.tombstone_slot(0), Err(Error::SlotDeleted(0))));
}

#[test]
fn slots_that_were_never_handed_out_are_an_error() {
    let mut page = slotted_page();
    assert!(matches!(page.slot_bytes(0), Err(Error::NoSuchSlot(0))));

    page.append_slot(b"only one").unwrap();
    assert!(matches!(page.slot_bytes(1), Err(Error::NoSuchSlot(1))));
    assert!(matches!(page.tombstone_slot(9), Err(Error::NoSuchSlot(9))));
}

#[test]
fn a_record_can_fill_the_page_exactly() {
    let mut page = slotted_page();
    let biggest = page.free_space() - SLOT_SIZE;

    page.append_slot(&vec![7u8; biggest]).unwrap();

    assert_eq!(page.free_space(), 0);
    assert_eq!(page.slot_bytes(0).unwrap().len(), biggest);
    assert!(matches!(
        page.append_slot(b"x"),
        Err(Error::PageFull { .. })
    ));
}

#[test]
fn inserting_until_full_leaves_every_record_intact() {
    let mut page = slotted_page();
    let mut ids = Vec::new();

    loop {
        let record = vec![ids.len() as u8; 100];
        match page.append_slot(&record) {
            Ok(slot) => ids.push(slot),
            Err(Error::PageFull { .. }) => break,
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    assert!(ids.len() > 30, "expected a full page, got {}", ids.len());
    for (i, &slot) in ids.iter().enumerate() {
        assert_eq!(page.slot_bytes(slot).unwrap(), &vec![i as u8; 100][..]);
    }
}

#[test]
fn stored_free_space_matches_the_derived_value() {
    // the price of caching free space in the header is that it can drift
    let mut page = slotted_page();
    assert_eq!(page.free_space(), derived_free_space(&page));

    for i in 0..5usize {
        page.append_slot(&vec![i as u8; 40 + i]).unwrap();
        assert_eq!(page.free_space(), derived_free_space(&page));
    }

    page.tombstone_slot(2).unwrap();
    assert_eq!(page.free_space(), derived_free_space(&page));
}

// the records in slot order, so a test can state the whole page in one line
fn records(page: &Page) -> Vec<Vec<u8>> {
    (0..page.slot_count())
        .map(|slot| page.slot_bytes(slot).unwrap().to_vec())
        .collect()
}

fn page_with(records: &[&[u8]]) -> Page {
    let mut page = slotted_page();
    for record in records {
        page.append_slot(record).unwrap();
    }
    page
}

#[test]
fn inserting_at_the_front_pushes_everything_right() {
    let mut page = page_with(&[b"b", b"c"]);

    page.insert_slot_at(0, b"a").unwrap();

    assert_eq!(records(&page), vec![b"a", b"b", b"c"]);
    assert_eq!(page.slot_count(), 3);
}

#[test]
fn inserting_in_the_middle_shifts_only_the_tail() {
    let mut page = page_with(&[b"a", b"c", b"d"]);

    page.insert_slot_at(1, b"b").unwrap();

    assert_eq!(records(&page), vec![b"a", b"b", b"c", b"d"]);
}

#[test]
fn inserting_at_slot_count_appends() {
    // this is the position search_slot returns for a key past the last one
    let mut page = page_with(&[b"a", b"b"]);

    page.insert_slot_at(2, b"c").unwrap();

    assert_eq!(records(&page), vec![b"a", b"b", b"c"]);
}

#[test]
fn inserting_past_the_end_is_an_error() {
    let mut page = page_with(&[b"a", b"b"]);

    assert!(matches!(
        page.insert_slot_at(3, b"x"),
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

    page.insert_slot_at(0, b"a").unwrap();

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
        page.insert_slot_at(at as SlotId, record).unwrap();
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
    page.append_slot(&vec![7u8; biggest]).unwrap();

    assert!(matches!(
        page.insert_slot_at(0, b"x"),
        Err(Error::PageFull { .. })
    ));
    assert_eq!(page.slot_count(), 1);
}

#[test]
fn free_space_stays_consistent_after_positional_inserts() {
    let mut page = slotted_page();

    for i in 0..6usize {
        page.insert_slot_at(0, &vec![i as u8; 30 + i]).unwrap();
        assert_eq!(page.free_space(), derived_free_space(&page));
    }
}

#[test]
fn removing_the_front_pulls_everything_left() {
    let mut page = page_with(&[b"a", b"b", b"c"]);

    page.remove_slot_at(0).unwrap();

    assert_eq!(records(&page), vec![b"b", b"c"]);
    assert_eq!(page.slot_count(), 2);
}

#[test]
fn removing_the_middle_closes_the_gap() {
    // no tombstone, unlike tombstone_slot: the array stays dense
    let mut page = page_with(&[b"a", b"b", b"c", b"d"]);

    page.remove_slot_at(1).unwrap();

    assert_eq!(records(&page), vec![b"a", b"c", b"d"]);
}

#[test]
fn removing_the_last_shifts_nothing() {
    let mut page = page_with(&[b"a", b"b", b"c"]);

    page.remove_slot_at(2).unwrap();

    assert_eq!(records(&page), vec![b"a", b"b"]);
}

#[test]
fn removing_past_the_end_is_an_error() {
    let mut page = page_with(&[b"a", b"b"]);

    assert!(matches!(page.remove_slot_at(2), Err(Error::NoSuchSlot(2))));
    assert_eq!(records(&page), vec![b"a", b"b"]);
}

#[test]
fn removing_strands_the_record_bytes_but_frees_the_slot() {
    // sizes differ so a wrong slot's length would show up
    let mut page = page_with(&[b"aa", b"bbbb", b"cccccc"]);
    let free_before = page.free_space();
    let free_ptr_before = page.read_u16(OFF_FREE_PTR);

    page.remove_slot_at(1).unwrap();

    assert_eq!(page.frag_space(), 4);
    assert_eq!(page.free_space(), free_before + SLOT_SIZE);
    // the data region never moved, that is what makes those 4 bytes stranded
    assert_eq!(page.read_u16(OFF_FREE_PTR), free_ptr_before);
}

#[test]
fn fragmentation_adds_up_over_several_removals() {
    let mut page = page_with(&[b"aa", b"bbbb", b"cccccc"]);

    page.remove_slot_at(0).unwrap();
    assert_eq!(page.frag_space(), 2);
    page.remove_slot_at(1).unwrap();
    assert_eq!(page.frag_space(), 8);

    assert_eq!(records(&page), vec![b"bbbb"]);
}

#[test]
fn removing_every_record_leaves_an_empty_page() {
    let mut page = page_with(&[b"a", b"b", b"c"]);

    for _ in 0..3 {
        page.remove_slot_at(0).unwrap();
    }

    assert_eq!(page.slot_count(), 0);
    assert!(matches!(page.slot_bytes(0), Err(Error::NoSuchSlot(0))));
    assert!(matches!(page.remove_slot_at(0), Err(Error::NoSuchSlot(0))));
}

#[test]
fn free_space_stays_consistent_across_removals() {
    let mut page = slotted_page();
    for i in 0..6usize {
        page.append_slot(&vec![i as u8; 30 + i]).unwrap();
    }

    while page.slot_count() > 0 {
        page.remove_slot_at(0).unwrap();
        assert_eq!(page.free_space(), derived_free_space(&page));
    }
}

#[test]
fn insert_after_remove_keeps_the_order() {
    // the tree does exactly this, remove a key then put another in its place
    let mut page = page_with(&[b"a", b"c", b"e"]);

    page.remove_slot_at(1).unwrap();
    page.insert_slot_at(1, b"b").unwrap();

    assert_eq!(records(&page), vec![b"a", b"b", b"e"]);
}

#[test]
fn compacting_hands_back_the_stranded_bytes() {
    let mut page = page_with(&[b"aaaa", b"bbbb", b"cccc", b"dddd"]);

    page.remove_slot_at(1).unwrap();
    page.remove_slot_at(1).unwrap();
    assert_eq!(page.frag_space(), 8);

    let before = page.free_space();
    page.compact();

    assert_eq!(page.frag_space(), 0);
    assert_eq!(page.free_space(), before + 8);
    assert_eq!(page.free_space(), derived_free_space(&page));
}

#[test]
fn compacting_leaves_the_records_alone() {
    let mut page = page_with(&[b"first", b"second", b"third", b"fourth"]);

    page.remove_slot_at(0).unwrap();
    page.compact();

    assert_eq!(
        records(&page),
        vec![b"second".to_vec(), b"third".to_vec(), b"fourth".to_vec()]
    );
}

#[test]
fn compacting_an_untouched_page_changes_nothing() {
    let mut page = page_with(&[b"a", b"bb", b"ccc"]);
    let before = page.free_space();

    page.compact();

    assert_eq!(
        records(&page),
        vec![b"a".to_vec(), b"bb".to_vec(), b"ccc".to_vec()]
    );
    assert_eq!(page.free_space(), before);
}

#[test]
fn compacting_keeps_the_page_type_and_link() {
    let mut page = page_with(&[b"one", b"two"]);
    page.set_page_type(7);
    page.set_link(42);

    page.remove_slot_at(0).unwrap();
    page.compact();

    assert_eq!(page.page_type(), 7);
    assert_eq!(page.link(), 42);
}

#[test]
fn compacting_keeps_tombstones_in_place() {
    // heap pages hand out SlotIds, so a tombstone cannot lose its slot
    let mut page = page_with(&[b"keep", b"gone", b"also keep"]);

    page.tombstone_slot(1).unwrap();
    page.compact();

    assert_eq!(page.slot_count(), 3);
    assert_eq!(page.slot_bytes(0).unwrap(), b"keep");
    assert!(matches!(page.slot_bytes(1), Err(Error::SlotDeleted(1))));
    assert_eq!(page.slot_bytes(2).unwrap(), b"also keep");
}
