use std::fs::{File, OpenOptions};
use std::os::unix::fs::FileExt;
use std::path::Path;

use crate::error::{Error, Result};
use crate::page::{PAGE_SIZE, Page, PageId};

pub struct DiskManager {
    // File is not the file's contents. It's a handle.
    file: File,
}

impl DiskManager {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true) // create if missing
            .truncate(false) // never wipe an existing database
            .open(path)?; // returns the file handle

        Ok(Self { file })
    }

    pub fn read_page(&self, page_id: PageId) -> Result<Page> {
        let page_count = self.page_count()?;
        if page_id.0 >= page_count {
            return Err(Error::PageOutOfRange {
                requested: page_id.0,
                page_count,
            });
        }

        let mut buf = [0u8; PAGE_SIZE];
        self.file.read_exact_at(&mut buf, page_id.offset())?;
        Ok(Page::from_bytes(buf))
    }

    // Writes page bytes into the file (OS cache).
    // call sync() to make them durable on disk.
    pub fn write_page(&self, page_id: PageId, page: &Page) -> Result<()> {
        self.file.write_all_at(page.as_bytes(), page_id.offset())?;
        Ok(())
    }

    pub fn allocate_page(&mut self) -> Result<PageId> {
        let page_count = self.page_count()?;
        // ids are 0-based, so with N pages the next free id is N
        let new_id = PageId(page_count);
        let page = Page::new();
        self.write_page(new_id, &page)?;
        Ok(new_id)
    }

    // Flushes the file from OS cache to physical disk so writes survive a crash.
    pub fn sync(&self) -> Result<()> {
        self.file.sync_all()?;
        Ok(())
    }

    pub fn page_count(&self) -> Result<u64> {
        Ok(self.file.metadata()?.len() / PAGE_SIZE as u64)
    }
}

#[cfg(test)]
mod tests {
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
}
