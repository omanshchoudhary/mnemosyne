#![allow(dead_code)]

use std::path::Path;

use crate::btree::node::encode_leaf_entry;
use crate::buffer::{BufferPool, FrameId};
use crate::error::{Error, Result};
use crate::page::{PageId, RecordId};

// 0 reserved for MetaPage
const META_PAGE_ID: PageId = PageId(0);

pub struct BTree {
    pool: BufferPool,
}

impl BTree {
    pub fn open(path: &Path, frame_count: usize) -> Result<Self> {
        let mut pool = BufferPool::open(path, frame_count)?;

        if pool.page_count()? == 0 {
            // allocation is sequential from an empty file
            let (_meta_id, meta_frame) = pool.new_page()?; // page 0
            let (root_id, root_frame) = pool.new_page()?; // page 1

            pool.page_mut(root_frame).init_leaf();
            pool.page_mut(meta_frame).init_meta(root_id);

            pool.unpin(root_frame)?;
            pool.unpin(meta_frame)?;
            pool.flush_all()?;
        } else {
            let meta_frame = pool.fetch_page(META_PAGE_ID)?;
            let valid = pool.page(meta_frame).is_meta();
            pool.unpin(meta_frame)?;

            if !valid {
                return Err(Error::BadMetaPage);
            }
        }

        Ok(Self { pool })
    }

    fn root(&mut self) -> Result<PageId> {
        let frame = self.pool.fetch_page(META_PAGE_ID)?;
        let root = self.pool.page(frame).root_page_id();
        self.pool.unpin(frame)?;
        Ok(root)
    }

    fn set_root(&mut self, root: PageId) -> Result<()> {
        let frame = self.pool.fetch_page(META_PAGE_ID)?;
        self.pool.page_mut(frame).set_root_page_id(root);
        self.pool.unpin(frame)?;
        Ok(())
    }

    fn find_leaf(&mut self, key: &[u8]) -> Result<FrameId> {
        let mut page_id = self.root()?;

        loop {
            let frame = self.pool.fetch_page(page_id)?;
            if self.pool.page(frame).is_leaf() {
                return Ok(frame); // still pinned, on purpose
            }
            let child = self.pool.page(frame).child_for_key(key)?;
            self.pool.unpin(frame)?;
            page_id = child;
        }
    }

    pub fn insert(&mut self, key: &[u8], record: RecordId) -> Result<()> {
        let frame = self.find_leaf(key)?;

        let (found, slot) = self.pool.page(frame).search_slot(key)?;

        let result = if found {
            self.pool.page_mut(frame).set_leaf_record_id(slot, record)
        } else {
            let entry = encode_leaf_entry(record, key);
            self.pool.page_mut(frame).insert_record_at(slot, &entry)
        };
        self.pool.unpin(frame)?;
        result
    }

    pub fn lookup(&mut self, key: &[u8]) -> Result<Option<RecordId>> {
        let frame = self.find_leaf(key)?;

        let (found, slot) = self.pool.page(frame).search_slot(key)?;
        let record = if found {
            Some(self.pool.page(frame).leaf_record_id(slot)?)
        } else {
            None
        };

        self.pool.unpin(frame)?;
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const FRAMES: usize = 8;

    // the TempDir has to outlive the tree, or the file is deleted underneath it
    fn temp_db() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tree.db");
        (dir, path)
    }

    fn rid(page: u64, slot: u16) -> RecordId {
        RecordId {
            page: PageId(page),
            slot,
        }
    }

    // how many entries the root leaf is holding
    fn root_slot_count(tree: &mut BTree) -> u16 {
        let root = tree.root().unwrap();
        let frame = tree.pool.fetch_page(root).unwrap();
        let count = tree.pool.page(frame).slot_count();
        tree.pool.unpin(frame).unwrap();
        count
    }

    #[test]
    fn a_fresh_file_gets_a_meta_page_and_an_empty_root_leaf() {
        let (_dir, path) = temp_db();
        let mut tree = BTree::open(&path, FRAMES).unwrap();

        // meta is page 0, so the first page the tree can use is 1
        assert_eq!(tree.root().unwrap(), PageId(1));

        let frame = tree.pool.fetch_page(PageId(1)).unwrap();
        let root = tree.pool.page(frame);
        assert!(root.is_leaf());
        assert_eq!(root.slot_count(), 0);
        assert_eq!(root.next_leaf(), None);
        tree.pool.unpin(frame).unwrap();
    }

    #[test]
    fn opening_a_fresh_file_writes_both_pages_to_disk() {
        // open flushes, so the two pages must be on disk before it returns
        let (_dir, path) = temp_db();
        {
            BTree::open(&path, FRAMES).unwrap();
        }

        let len = std::fs::metadata(&path).unwrap().len();
        assert_eq!(len, 2 * crate::page::PAGE_SIZE as u64);
    }

    #[test]
    fn reopening_finds_the_same_root() {
        let (_dir, path) = temp_db();
        {
            let mut tree = BTree::open(&path, FRAMES).unwrap();
            assert_eq!(tree.root().unwrap(), PageId(1));
        }

        // the second open must take the existing path, not bootstrap again
        let mut tree = BTree::open(&path, FRAMES).unwrap();
        assert_eq!(tree.root().unwrap(), PageId(1));
        assert_eq!(tree.pool.page_count().unwrap(), 2);
    }

    #[test]
    fn a_new_root_survives_a_reopen() {
        // what a root split does: point the meta page somewhere else
        let (_dir, path) = temp_db();
        {
            let mut tree = BTree::open(&path, FRAMES).unwrap();
            tree.set_root(PageId(7)).unwrap();
            tree.pool.flush_all().unwrap();
        }

        let mut tree = BTree::open(&path, FRAMES).unwrap();
        assert_eq!(tree.root().unwrap(), PageId(7));
    }

    #[test]
    fn a_file_that_is_not_a_database_is_rejected() {
        let (_dir, path) = temp_db();
        // one page of junk, so page_count is not 0 and the magic check runs
        std::fs::write(&path, vec![0xABu8; crate::page::PAGE_SIZE]).unwrap();

        assert!(matches!(
            BTree::open(&path, FRAMES),
            Err(Error::BadMetaPage)
        ));
    }

    #[test]
    fn open_leaves_no_frame_pinned() {
        // a pin leak would only show up much later, as a full pool
        let (_dir, path) = temp_db();
        let mut tree = BTree::open(&path, 2).unwrap();

        // both frames hold open's pages, so allocating two more forces an
        // eviction each, which is only possible if open unpinned them
        for _ in 0..2 {
            let (_, frame) = tree.pool.new_page().unwrap();
            tree.pool.unpin(frame).unwrap();
        }
    }

    #[test]
    fn a_key_comes_back_after_being_inserted() {
        let (_dir, path) = temp_db();
        let mut tree = BTree::open(&path, FRAMES).unwrap();

        tree.insert(b"apple", rid(57, 3)).unwrap();

        assert_eq!(tree.lookup(b"apple").unwrap(), Some(rid(57, 3)));
    }

    #[test]
    fn a_key_that_was_never_inserted_is_none() {
        let (_dir, path) = temp_db();
        let mut tree = BTree::open(&path, FRAMES).unwrap();

        // empty tree, and a tree holding a neighbour on either side
        assert_eq!(tree.lookup(b"nothing").unwrap(), None);

        tree.insert(b"b", rid(1, 0)).unwrap();
        assert_eq!(tree.lookup(b"a").unwrap(), None);
        assert_eq!(tree.lookup(b"c").unwrap(), None);
    }

    #[test]
    fn keys_inserted_out_of_order_all_come_back() {
        // the leaf must end up sorted however they arrive, or search misses
        let (_dir, path) = temp_db();
        let mut tree = BTree::open(&path, FRAMES).unwrap();

        let keys: [&[u8]; 7] = [b"m", b"c", b"t", b"a", b"z", b"f", b"q"];
        for (i, key) in keys.iter().enumerate() {
            tree.insert(key, rid(9, i as u16)).unwrap();
        }

        for (i, key) in keys.iter().enumerate() {
            assert_eq!(tree.lookup(key).unwrap(), Some(rid(9, i as u16)));
        }
        assert_eq!(root_slot_count(&mut tree), 7);
    }

    #[test]
    fn inserting_the_same_key_twice_overwrites_it() {
        let (_dir, path) = temp_db();
        let mut tree = BTree::open(&path, FRAMES).unwrap();

        tree.insert(b"key", rid(1, 1)).unwrap();
        tree.insert(b"key", rid(2, 2)).unwrap();

        assert_eq!(tree.lookup(b"key").unwrap(), Some(rid(2, 2)));
        // an overwrite is an edit, not a second entry
        assert_eq!(root_slot_count(&mut tree), 1);
    }

    #[test]
    fn an_overwrite_leaves_its_neighbours_alone() {
        // the rewrite is 10 bytes inside one entry, so nothing else can move
        let (_dir, path) = temp_db();
        let mut tree = BTree::open(&path, FRAMES).unwrap();

        tree.insert(b"a", rid(1, 0)).unwrap();
        tree.insert(b"b", rid(2, 0)).unwrap();
        tree.insert(b"c", rid(3, 0)).unwrap();

        tree.insert(b"b", rid(99, 9)).unwrap();

        assert_eq!(tree.lookup(b"a").unwrap(), Some(rid(1, 0)));
        assert_eq!(tree.lookup(b"b").unwrap(), Some(rid(99, 9)));
        assert_eq!(tree.lookup(b"c").unwrap(), Some(rid(3, 0)));
        assert_eq!(root_slot_count(&mut tree), 3);
    }

    #[test]
    fn inserted_keys_survive_a_reopen() {
        let (_dir, path) = temp_db();
        {
            let mut tree = BTree::open(&path, FRAMES).unwrap();
            tree.insert(b"persisted", rid(4, 2)).unwrap();
            tree.pool.flush_all().unwrap();
        }

        let mut tree = BTree::open(&path, FRAMES).unwrap();
        assert_eq!(tree.lookup(b"persisted").unwrap(), Some(rid(4, 2)));
    }

    #[test]
    fn insert_leaves_no_frame_pinned() {
        let (_dir, path) = temp_db();
        let mut tree = BTree::open(&path, 3).unwrap();

        // more inserts than frames, so a leaked pin per insert fills the pool
        for i in 0..20u16 {
            tree.insert(format!("key{i:03}").as_bytes(), rid(1, i))
                .unwrap();
        }

        assert_eq!(tree.lookup(b"key019").unwrap(), Some(rid(1, 19)));
    }

    #[test]
    fn filling_the_root_leaf_reports_page_full() {
        // splitting is not implemented yet, so this is the current boundary
        let (_dir, path) = temp_db();
        let mut tree = BTree::open(&path, FRAMES).unwrap();

        let mut inserted = 0u16;
        loop {
            let key = format!("{inserted:04}");
            match tree.insert(key.as_bytes(), rid(1, inserted)) {
                Ok(()) => inserted += 1,
                Err(Error::PageFull { .. }) => break,
                Err(e) => panic!("unexpected error: {e}"),
            }
        }

        assert!(inserted > 100, "expected a full leaf, got {inserted}");
        // everything that did fit is still findable
        assert_eq!(tree.lookup(b"0000").unwrap(), Some(rid(1, 0)));
        assert_eq!(
            tree.lookup(format!("{:04}", inserted - 1).as_bytes())
                .unwrap(),
            Some(rid(1, inserted - 1))
        );
    }
}
