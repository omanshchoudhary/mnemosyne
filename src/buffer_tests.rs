use super::*;
use tempfile::TempDir;

fn temp_db() -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    (dir, path)
}

#[test]
fn new_page_hands_out_ascending_ids() {
    let (_dir, path) = temp_db();
    let mut pool = BufferPool::open(&path, 4).unwrap();

    let (first, _) = pool.new_page().unwrap();
    let (second, _) = pool.new_page().unwrap();

    assert_eq!(first, PageId(0));
    assert_eq!(second, PageId(1));
}

#[test]
fn a_written_page_reads_back_through_the_pool() {
    let (_dir, path) = temp_db();
    let mut pool = BufferPool::open(&path, 4).unwrap();

    let (page_id, frame_id) = pool.new_page().unwrap();
    pool.page_mut(frame_id).write_u64(0, 0xCAFE);
    pool.unpin(frame_id).unwrap();

    let again = pool.fetch_page(page_id).unwrap();
    assert_eq!(pool.page(again).read_u64(0), 0xCAFE);
}

#[test]
fn fetching_a_cached_page_reuses_its_frame() {
    let (_dir, path) = temp_db();
    let mut pool = BufferPool::open(&path, 4).unwrap();

    let (page_id, frame_id) = pool.new_page().unwrap();
    pool.unpin(frame_id).unwrap();

    assert_eq!(pool.fetch_page(page_id).unwrap(), frame_id);
    assert_eq!(pool.free_list.len(), 3);
}

#[test]
fn a_dirty_page_survives_eviction() {
    let (_dir, path) = temp_db();
    let mut pool = BufferPool::open(&path, 2).unwrap();

    let (first, frame_id) = pool.new_page().unwrap();
    pool.page_mut(frame_id).write_u64(0, 42);
    pool.unpin(frame_id).unwrap();

    for _ in 0..2 {
        let (_, frame_id) = pool.new_page().unwrap();
        pool.unpin(frame_id).unwrap();
    }

    assert!(!pool.page_table.contains_key(&first));

    let frame_id = pool.fetch_page(first).unwrap();
    assert_eq!(pool.page(frame_id).read_u64(0), 42);
}

#[test]
fn a_new_page_never_inherits_the_old_frames_bytes() {
    let (_dir, path) = temp_db();
    let mut pool = BufferPool::open(&path, 1).unwrap();

    let (_, frame_id) = pool.new_page().unwrap();
    pool.page_mut(frame_id).write_u64(0, 0xFFFF);
    pool.unpin(frame_id).unwrap();

    let (_, frame_id) = pool.new_page().unwrap();
    assert_eq!(pool.page(frame_id).read_u64(0), 0);
}

#[test]
fn a_fully_pinned_pool_cannot_take_another_page() {
    let (_dir, path) = temp_db();
    let mut pool = BufferPool::open(&path, 2).unwrap();

    pool.new_page().unwrap();
    pool.new_page().unwrap();

    assert!(matches!(pool.new_page(), Err(Error::BufferPoolFull)));
}

#[test]
fn unpinning_lets_a_frame_be_reused() {
    let (_dir, path) = temp_db();
    let mut pool = BufferPool::open(&path, 1).unwrap();

    let (_, frame_id) = pool.new_page().unwrap();
    assert!(matches!(pool.new_page(), Err(Error::BufferPoolFull)));

    pool.unpin(frame_id).unwrap();
    assert!(pool.new_page().is_ok());
}

#[test]
fn a_page_pinned_twice_needs_two_unpins() {
    let (_dir, path) = temp_db();
    let mut pool = BufferPool::open(&path, 1).unwrap();

    let (page_id, frame_id) = pool.new_page().unwrap();
    assert_eq!(pool.fetch_page(page_id).unwrap(), frame_id);

    pool.unpin(frame_id).unwrap();
    assert!(matches!(pool.new_page(), Err(Error::BufferPoolFull)));

    pool.unpin(frame_id).unwrap();
    assert!(pool.new_page().is_ok());
}

#[test]
fn unpinning_twice_is_an_error() {
    let (_dir, path) = temp_db();
    let mut pool = BufferPool::open(&path, 2).unwrap();

    let (_, frame_id) = pool.new_page().unwrap();
    pool.unpin(frame_id).unwrap();

    assert!(matches!(
        pool.unpin(frame_id),
        Err(Error::FrameNotPinned(_))
    ));
}

#[test]
fn flush_all_puts_everything_on_disk() {
    let (_dir, path) = temp_db();

    let page_id = {
        let mut pool = BufferPool::open(&path, 4).unwrap();
        let (page_id, frame_id) = pool.new_page().unwrap();
        pool.page_mut(frame_id).write_u64(0, 0xDEAD_BEEF);
        pool.unpin(frame_id).unwrap();
        pool.flush_all().unwrap();
        page_id
    };

    let mut pool = BufferPool::open(&path, 4).unwrap();
    let frame_id = pool.fetch_page(page_id).unwrap();
    assert_eq!(pool.page(frame_id).read_u64(0), 0xDEAD_BEEF);
}

#[test]
fn dropping_the_pool_without_flushing_loses_the_change() {
    let (_dir, path) = temp_db();

    let page_id = {
        let mut pool = BufferPool::open(&path, 4).unwrap();
        let (page_id, frame_id) = pool.new_page().unwrap();
        pool.page_mut(frame_id).write_u64(0, 7);
        pool.unpin(frame_id).unwrap();
        page_id
    };

    let mut pool = BufferPool::open(&path, 4).unwrap();
    let frame_id = pool.fetch_page(page_id).unwrap();
    assert_eq!(pool.page(frame_id).read_u64(0), 0);
}
