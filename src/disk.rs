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
#[path = "disk_tests.rs"]
mod tests;
