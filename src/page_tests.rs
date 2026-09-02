use super::*;

#[test]
fn new_page_is_all_zeros() {
    let page = Page::new();
    assert_eq!(page.data.len(), PAGE_SIZE);
    assert!(page.data.iter().all(|&b| b == 0));
}

#[test]
fn u16_round_trip() {
    let mut page = Page::new();
    page.write_u16(0, 1);
    page.write_u16(100, u16::MAX);
    assert_eq!(page.read_u16(0), 1);
    assert_eq!(page.read_u16(100), u16::MAX);
}

#[test]
fn u32_round_trip() {
    let mut page = Page::new();
    page.write_u32(8, 123_456_789);
    assert_eq!(page.read_u32(8), 123_456_789);
}

#[test]
fn u64_round_trip() {
    let mut page = Page::new();
    page.write_u64(16, u64::MAX);
    page.write_u64(32, 0);
    assert_eq!(page.read_u64(16), u64::MAX);
    assert_eq!(page.read_u64(32), 0);
}

#[test]
fn integers_are_stored_little_endian() {
    // the low byte lands first; this is what catches a be/le mismatch
    let mut page = Page::new();
    page.write_u16(0, 0x0102);
    assert_eq!(&page.data[0..2], &[0x02, 0x01]);

    page.write_u32(4, 0x0102_0304);
    assert_eq!(&page.data[4..8], &[0x04, 0x03, 0x02, 0x01]);

    page.write_u64(8, 0x0102_0304_0506_0708);
    assert_eq!(
        &page.data[8..16],
        &[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]
    );
}

#[test]
fn writes_do_not_touch_neighbouring_bytes() {
    let mut page = Page::new();
    page.write_u16(10, u16::MAX);
    assert_eq!(page.data[9], 0);
    assert_eq!(page.data[12], 0);
}

#[test]
fn bytes_round_trip() {
    let mut page = Page::new();
    let record = b"hello mnemosyne";
    page.write_bytes(64, record);
    assert_eq!(page.read_bytes(64, record.len()), record);
}

#[test]
fn overwriting_replaces_the_old_value() {
    let mut page = Page::new();
    page.write_u32(0, 42);
    page.write_u32(0, 7);
    assert_eq!(page.read_u32(0), 7);
}

#[test]
fn page_id_maps_to_a_file_offset() {
    assert_eq!(PageId(0).offset(), 0);
    assert_eq!(PageId(1).offset(), PAGE_SIZE as u64);
    assert_eq!(PageId(5).offset(), 20_480);
}

#[test]
#[should_panic]
fn reading_past_the_end_panics() {
    let page = Page::new();
    page.read_u32(PAGE_SIZE - 2);
}
