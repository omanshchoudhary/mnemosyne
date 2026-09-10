use std::path::Path;

use crate::btree::BTree;
use crate::error::Result;
use crate::heap;
use crate::mvcc::txn::{Txn, TxnManager};
use crate::mvcc::version::{
    encode_version, set_version_end, version_begin, version_end, version_prev, version_value,
};
use crate::page::meta::META_PAGE_ID;

pub struct Db {
    tree: BTree,
    txns: TxnManager,
}

impl Db {
    // opens the db file/creates and resumes the timestamp counter
    pub fn open(path: &Path, frame_count: usize) -> Result<Self> {
        let mut tree = BTree::open(path, frame_count)?;

        let pool = tree.pool();
        let frame = pool.fetch_and_pin(META_PAGE_ID)?;
        let next_timestamp = pool.page(frame).next_timestamp();
        pool.unpin(frame)?;

        Ok(Self {
            tree,
            txns: TxnManager::new(next_timestamp),
        })
    }
    // starts a txn
    pub fn begin(&mut self) -> Result<Txn> {
        let txn = self.txns.begin();

        let pool = self.tree.pool();
        let frame = pool.fetch_and_pin(META_PAGE_ID)?;
        pool.page_for_write(frame)
            .set_next_timestamp(txn.timestamp + 1);
        pool.unpin(frame)?;

        Ok(txn)
    }

    // newest version of key this txn can see
    pub fn get(&mut self, txn: &Txn, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let Some(mut rid) = self.tree.lookup(key)? else {
            return Ok(None);
        };

        loop {
            let version = heap::get(self.tree.pool(), rid)?;

            if txn.sees_version(version_begin(&version), version_end(&version)) {
                return Ok(Some(version_value(&version).to_vec()));
            }

            match version_prev(&version) {
                Some(older) => rid = older,
                None => return Ok(None),
            }
        }
    }

    // writes a new version of key stamped with this txn's timestamp
    pub fn put(&mut self, txn: &mut Txn, key: &[u8], value: &[u8]) -> Result<()> {
        let head = self.tree.lookup(key)?;
        let version = encode_version(txn.timestamp, head, value);
        let new_rid = heap::insert(self.tree.pool(), &version)?;

        if let Some(old_rid) = head {
            let mut old = heap::get(self.tree.pool(), old_rid)?;
            set_version_end(&mut old, txn.timestamp);
            heap::overwrite(self.tree.pool(), old_rid, &old)?;
        }

        self.tree.insert(key, new_rid)
    }

    // txn stops running, so txns that begin after this can see its writes
    pub fn commit(&mut self, txn: Txn) {
        self.txns.finish(txn.timestamp);
    }

    // writes every dirty page to disk
    pub fn close(mut self) -> Result<()> {
        self.tree.pool().flush_all()
    }
}
