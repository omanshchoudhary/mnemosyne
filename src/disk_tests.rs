use super::*;
use tempfile::TempDir;

// The TempDir deletes itself when dropped, so tests must hold on to it for
// as long as they use the path. Binding it to `_dir` keeps it alive; a bare
// `_` would drop it immediately and delete the directory out from under us.
fn temp_db() -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    (dir, path)
}

// A page whose first 8 bytes identify it, so a test can tell pages apart.
fn page_marked(marker: u64) -> Page {
    let mut page = Page::new();
    page.write_u64(0, marker);
    page
}

#[test]
fn allocate_page_starts_at_zero() {
    let (_dir, path) = temp_db();
    let mut disk = DiskManager::open(&path).unwrap();

    assert_eq!(disk.allocate_page().unwrap(), PageId(0));
}

#[test]
fn allocate_page_never_hands_out_the_same_id_twice() {
    let (_dir, path) = temp_db();
    let mut disk = DiskManager::open(&path).unwrap();

    let ids: Vec<PageId> = (0..5).map(|_| disk.allocate_page().unwrap()).collect();

    assert_eq!(
        ids,
        vec![PageId(0), PageId(1), PageId(2), PageId(3), PageId(4)]
    );
}

#[test]
fn allocate_page_grows_the_file() {
    let (_dir, path) = temp_db();
    let mut disk = DiskManager::open(&path).unwrap();

    assert_eq!(disk.page_count().unwrap(), 0);
    disk.allocate_page().unwrap();
    assert_eq!(disk.page_count().unwrap(), 1);
    disk.allocate_page().unwrap();
    assert_eq!(disk.page_count().unwrap(), 2);
}

#[test]
fn each_page_round_trips_its_own_content() {
    // This is the test that pins down PageId::offset(). If the offset maths
    // were wrong, every page would still round trip when they all hold the
    // same bytes -- only distinct content catches it.
    let (_dir, path) = temp_db();
    let mut disk = DiskManager::open(&path).unwrap();

    for marker in 0..4u64 {
        let id = disk.allocate_page().unwrap();
        disk.write_page(id, &page_marked(marker + 100)).unwrap();
    }

    for marker in 0..4u64 {
        let page = disk.read_page(PageId(marker)).unwrap();
        assert_eq!(
            page.read_u64(0),
            marker + 100,
            "page {marker} came back wrong"
        );
    }
}

#[test]
fn data_survives_reopening_the_file() {
    // The whole point of a storage engine: the bytes outlive the process.
    let (_dir, path) = temp_db();

    {
        let mut disk = DiskManager::open(&path).unwrap();
        let id = disk.allocate_page().unwrap();
        disk.write_page(id, &page_marked(0xDEAD_BEEF)).unwrap();
        disk.sync().unwrap();
    } // disk dropped here, file closed

    let disk = DiskManager::open(&path).unwrap();
    assert_eq!(disk.page_count().unwrap(), 1);
    assert_eq!(disk.read_page(PageId(0)).unwrap().read_u64(0), 0xDEAD_BEEF);
}

#[test]
fn writing_past_the_end_leaves_a_hole_of_zeros() {
    let (_dir, path) = temp_db();
    let disk = DiskManager::open(&path).unwrap();

    disk.write_page(PageId(5), &page_marked(42)).unwrap();

    // the file now claims 6 pages, even though 0..4 were never written
    assert_eq!(disk.page_count().unwrap(), 6);
    assert_eq!(disk.read_page(PageId(2)).unwrap().read_u64(0), 0);
    assert_eq!(disk.read_page(PageId(5)).unwrap().read_u64(0), 42);
}

#[test]
fn read_page_beyond_file_returns_page_out_of_range() {
    let (_dir, path) = temp_db();

    let disk = DiskManager::open(&path).unwrap();
    assert_eq!(disk.page_count().unwrap(), 0);
    assert!(matches!(
        disk.read_page(PageId(0)),
        Err(Error::PageOutOfRange {
            requested: 0,
            page_count: 0
        })
    ));

    let page = Page::new();
    disk.write_page(PageId(0), &page).unwrap();
    assert_eq!(disk.page_count().unwrap(), 1);
    assert!(disk.read_page(PageId(0)).is_ok());
    assert!(matches!(
        disk.read_page(PageId(1)),
        Err(Error::PageOutOfRange {
            requested: 1,
            page_count: 1
        })
    ));
}
