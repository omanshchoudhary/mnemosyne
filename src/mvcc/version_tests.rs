use super::*;

fn rid(page: u64, slot: u16) -> RecordId {
    RecordId {
        page: PageId(page),
        slot,
    }
}

#[test]
fn the_header_is_26_bytes() {
    assert_eq!(VERSION_HEADER_SIZE, 26);
}

#[test]
fn a_version_is_its_header_plus_its_value() {
    let version = encode_version(1, None, b"hello");

    assert_eq!(version.len(), VERSION_HEADER_SIZE + 5);
}

#[test]
fn the_bytes_are_begin_then_end_then_prev_then_value() {
    let version = encode_version(7, Some(rid(3, 2)), b"v");

    assert_eq!(version[0..8], 7u64.to_le_bytes());
    assert_eq!(version[8..16], u64::MAX.to_le_bytes());
    assert_eq!(version[16..26], rid(3, 2).to_bytes());
    assert_eq!(version[26..], *b"v");
}

#[test]
fn begin_reads_back() {
    let version = encode_version(0x0102_0304_0506_0708, None, b"v");

    assert_eq!(version_begin(&version), 0x0102_0304_0506_0708);
}

#[test]
fn a_new_version_has_no_end() {
    let version = encode_version(7, None, b"v");

    assert_eq!(version_end(&version), NO_END);
}

#[test]
fn a_first_version_has_no_prev() {
    let version = encode_version(7, None, b"v");

    assert_eq!(version_prev(&version), None);
}

#[test]
fn prev_reads_back() {
    let version = encode_version(7, Some(rid(3, 2)), b"v");

    assert_eq!(version_prev(&version), Some(rid(3, 2)));
}

#[test]
fn a_prev_in_slot_zero_is_still_a_prev() {
    let version = encode_version(7, Some(rid(5, 0)), b"v");

    assert_eq!(version_prev(&version), Some(rid(5, 0)));
}

#[test]
fn the_value_reads_back() {
    let version = encode_version(7, Some(rid(3, 2)), b"hello");

    assert_eq!(version_value(&version), b"hello");
}

#[test]
fn an_empty_value_is_allowed() {
    let version = encode_version(7, None, b"");

    assert_eq!(version.len(), VERSION_HEADER_SIZE);
    assert_eq!(version_value(&version), b"");
}

#[test]
fn stamping_an_end_sets_it() {
    let mut version = encode_version(3, None, b"v");

    set_version_end(&mut version, 7);

    assert_eq!(version_end(&version), 7);
}

#[test]
fn stamping_an_end_leaves_everything_else_alone() {
    let mut version = encode_version(3, Some(rid(4, 1)), b"hello");

    set_version_end(&mut version, 7);

    assert_eq!(version_begin(&version), 3);
    assert_eq!(version_prev(&version), Some(rid(4, 1)));
    assert_eq!(version_value(&version), b"hello");
}
