use super::*;
use crate::page::slotted::{HEADER_SIZE, SLOT_SIZE};
use crate::page::{PAGE_SIZE, PageId};
use tempfile::TempDir;

const FRAMES: usize = 8;

const BIGGEST: usize = PAGE_SIZE - HEADER_SIZE - SLOT_SIZE;

fn temp_db() -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("heap.db");
    (dir, path)
}

fn fresh_pool(path: &std::path::Path, frames: usize) -> BufferPool {
    let mut pool = BufferPool::open(path, frames).unwrap();
    let (_, frame) = pool.new_page().unwrap();
    pool.page_for_write(frame).init_meta(PageId(0));
    pool.unpin(frame).unwrap();
    pool
}

fn tail(pool: &mut BufferPool) -> Option<PageId> {
    let frame = pool.fetch_and_pin(META_PAGE_ID).unwrap();
    let tail = pool.page(frame).heap_tail();
    pool.unpin(frame).unwrap();
    tail
}

#[test]
fn an_inserted_record_reads_back() {
    let (_dir, path) = temp_db();
    let mut pool = fresh_pool(&path, FRAMES);

    let rid = insert(&mut pool, b"hello").unwrap();

    assert_eq!(get(&mut pool, rid).unwrap(), b"hello");
}

#[test]
fn a_fresh_heap_has_no_tail() {
    let (_dir, path) = temp_db();
    let mut pool = fresh_pool(&path, FRAMES);

    assert_eq!(tail(&mut pool), None);
}

#[test]
fn the_first_insert_makes_the_page_after_meta_the_tail() {
    let (_dir, path) = temp_db();
    let mut pool = fresh_pool(&path, FRAMES);

    let rid = insert(&mut pool, b"first").unwrap();

    assert_eq!(
        rid,
        RecordId {
            page: PageId(1),
            slot: 0
        }
    );
    assert_eq!(tail(&mut pool), Some(PageId(1)));
}

#[test]
fn inserts_share_the_tail_page_while_it_has_room() {
    let (_dir, path) = temp_db();
    let mut pool = fresh_pool(&path, FRAMES);

    let first = insert(&mut pool, b"one").unwrap();
    let second = insert(&mut pool, b"two").unwrap();

    assert_eq!(second.page, first.page);
    assert_eq!(second.slot, first.slot + 1);
}

#[test]
fn an_insert_never_touches_an_earlier_record() {
    let (_dir, path) = temp_db();
    let mut pool = fresh_pool(&path, FRAMES);

    let first = insert(&mut pool, b"old").unwrap();
    insert(&mut pool, b"new").unwrap();

    assert_eq!(get(&mut pool, first).unwrap(), b"old");
}

#[test]
fn a_full_tail_moves_to_a_new_page() {
    let (_dir, path) = temp_db();
    let mut pool = fresh_pool(&path, FRAMES);

    let rids: Vec<RecordId> = (0..5u8)
        .map(|i| insert(&mut pool, &[i; 1000]).unwrap())
        .collect();

    assert_eq!(rids[3].page, rids[0].page);
    assert_ne!(rids[4].page, rids[0].page);
    assert_eq!(rids[4].slot, 0);
    assert_eq!(tail(&mut pool), Some(rids[4].page));
}

#[test]
fn many_records_across_many_pages_read_back() {
    let (_dir, path) = temp_db();
    let mut pool = fresh_pool(&path, 3);

    let records: Vec<Vec<u8>> = (0..300usize)
        .map(|i| format!("record {i} ").repeat(i % 7 + 1).into_bytes())
        .collect();
    let rids: Vec<RecordId> = records
        .iter()
        .map(|record| insert(&mut pool, record).unwrap())
        .collect();

    assert!(rids[rids.len() - 1].page > rids[0].page);
    for (rid, record) in rids.iter().zip(&records) {
        assert_eq!(&get(&mut pool, *rid).unwrap(), record);
    }
}

#[test]
fn a_record_that_fills_a_whole_page_fits() {
    let (_dir, path) = temp_db();
    let mut pool = fresh_pool(&path, FRAMES);

    let rid = insert(&mut pool, &[7u8; BIGGEST]).unwrap();

    assert_eq!(get(&mut pool, rid).unwrap(), [7u8; BIGGEST]);
}

#[test]
fn a_record_too_big_for_any_page_is_an_error() {
    let (_dir, path) = temp_db();
    let mut pool = fresh_pool(&path, FRAMES);

    assert!(matches!(
        insert(&mut pool, &[7u8; BIGGEST + 1]),
        Err(Error::PageFull { .. })
    ));
}

#[test]
fn an_insert_after_an_oversized_one_still_works() {
    let (_dir, path) = temp_db();
    let mut pool = fresh_pool(&path, FRAMES);

    insert(&mut pool, &[7u8; PAGE_SIZE]).unwrap_err();
    let rid = insert(&mut pool, b"fine").unwrap();

    assert_eq!(get(&mut pool, rid).unwrap(), b"fine");
}

#[test]
fn records_survive_a_reopen() {
    let (_dir, path) = temp_db();
    let rid = {
        let mut pool = fresh_pool(&path, FRAMES);
        let rid = insert(&mut pool, b"kept").unwrap();
        pool.flush_all().unwrap();
        rid
    };

    let mut pool = BufferPool::open(&path, FRAMES).unwrap();

    assert_eq!(get(&mut pool, rid).unwrap(), b"kept");
}

#[test]
fn the_tail_survives_a_reopen() {
    let (_dir, path) = temp_db();
    let first = {
        let mut pool = fresh_pool(&path, FRAMES);
        let rid = insert(&mut pool, b"before").unwrap();
        pool.flush_all().unwrap();
        rid
    };

    let mut pool = BufferPool::open(&path, FRAMES).unwrap();
    let second = insert(&mut pool, b"after").unwrap();

    assert_eq!(second.page, first.page);
}

#[test]
fn getting_a_slot_that_was_never_handed_out_is_an_error() {
    let (_dir, path) = temp_db();
    let mut pool = fresh_pool(&path, FRAMES);

    let rid = insert(&mut pool, b"only").unwrap();
    let missing = RecordId {
        slot: rid.slot + 1,
        ..rid
    };

    assert!(matches!(get(&mut pool, missing), Err(Error::NoSuchSlot(_))));
}

#[test]
fn insert_and_get_leave_no_frame_pinned() {
    let (_dir, path) = temp_db();
    let mut pool = fresh_pool(&path, 1);

    let rids: Vec<RecordId> = (0..200u8)
        .map(|i| insert(&mut pool, &[i; 100]).unwrap())
        .collect();

    for (i, rid) in rids.iter().enumerate() {
        assert_eq!(get(&mut pool, *rid).unwrap(), [i as u8; 100]);
    }
}

#[test]
fn a_failed_get_leaves_no_frame_pinned() {
    let (_dir, path) = temp_db();
    let mut pool = fresh_pool(&path, 1);

    let rid = insert(&mut pool, b"only").unwrap();
    get(&mut pool, RecordId { slot: 9, ..rid }).unwrap_err();

    insert(&mut pool, b"next").unwrap();
}
