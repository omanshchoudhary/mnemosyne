use std::path::Path;

use crate::btree::BTree;
use crate::error::{Error, Result};
use crate::heap;
use crate::mvcc::txn::{Txn, TxnManager};
use crate::mvcc::version::{
    NO_END, encode_version, set_version_end, set_version_prev, version_begin, version_end,
    version_prev, version_value,
};
use crate::page::RecordId;
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
        // old_rid and old_version bytes
        let old = match head {
            Some(old_rid) => Some((old_rid, heap::get(self.tree.pool(), old_rid)?)),
            None => None,
        };

        // if there is an old version and I can't replace it then fail.
        if let Some((_, old_version)) = &old
            && !txn.can_add_newer_version(version_begin(old_version), version_end(old_version))
        {
            return Err(Error::WriteConflict);
        }

        let version = encode_version(txn.timestamp, head, value);
        let new_rid = heap::insert(self.tree.pool(), &version)?;
        if let Some((old_rid, mut old_version)) = old
            && version_end(&old_version) == NO_END
        {
            set_version_end(&mut old_version, txn.timestamp);
            heap::overwrite(self.tree.pool(), old_rid, &old_version)?;
        }

        txn.writes.push(key.to_vec());
        self.tree.insert(key, new_rid)
    }

    // crosses out the head for this txn, older snapshots still read it
    pub fn delete(&mut self, txn: &mut Txn, key: &[u8]) -> Result<bool> {
        // old_rid and old_version bytes
        let Some(old_rid) = self.tree.lookup(key)? else {
            return Ok(false);
        };
        let mut old_version = heap::get(self.tree.pool(), old_rid)?;

        if !txn.can_add_newer_version(version_begin(&old_version), version_end(&old_version)) {
            return Err(Error::WriteConflict);
        }

        if version_end(&old_version) != NO_END {
            return Ok(false);
        }

        set_version_end(&mut old_version, txn.timestamp);
        heap::overwrite(self.tree.pool(), old_rid, &old_version)?;

        txn.writes.push(key.to_vec());
        Ok(true)
    }

    // undoes every write this txn made, then takes it out of the active set
    pub fn abort(&mut self, txn: Txn) -> Result<()> {
        for key in &txn.writes {
            let Some(mut rid) = self.tree.lookup(key)? else {
                continue;
            };

            // walk back past every version i wrote
            let restored = loop {
                let version = heap::get(self.tree.pool(), rid)?;
                if version_begin(&version) != txn.timestamp {
                    break Some((rid, version));
                }
                match version_prev(&version) {
                    Some(older) => rid = older,
                    None => break None,
                }
            };

            match restored {
                Some((old_rid, mut old_version)) => {
                    if version_end(&old_version) == txn.timestamp {
                        set_version_end(&mut old_version, NO_END);
                        heap::overwrite(self.tree.pool(), old_rid, &old_version)?;
                    }
                    self.tree.insert(key, old_rid)?;
                }
                // the key never existed before me
                None => {
                    self.tree.delete(key)?;
                }
            }
        }

        self.txns.finish(txn.timestamp);
        Ok(())
    }

    // frees every version no live or future txn can reach, returns how many
    pub fn gc(&mut self) -> Result<usize> {
        let mut count = 0;
        let watermark = self.txns.watermark();
        for (key, rid) in self.tree.keys()? {
            count += self.gc_key(&key, rid, watermark)?;
        }
        Ok(count)
    }

    // cuts one chain below the oldest version still readable
    fn gc_key(&mut self, key: &[u8], head: RecordId, watermark: u64) -> Result<usize> {
        let mut rid = head;
        let mut survivor = None;

        // dead is the first version nobody can reach, survivor is the live one above it
        let dead = loop {
            let version = heap::get(self.tree.pool(), rid)?;
            let end = version_end(&version);
            if end != NO_END && end < watermark {
                break Some(rid);
            }
            match version_prev(&version) {
                Some(older) => {
                    survivor = Some(rid);
                    rid = older;
                }
                None => break None,
            }
        };

        let Some(dead) = dead else {
            return Ok(0);
        };

        match survivor {
            Some(survivor) => {
                let mut version = heap::get(self.tree.pool(), survivor)?;
                set_version_prev(&mut version, None);
                heap::overwrite(self.tree.pool(), survivor, &version)?;
            }
            // the head itself was dead, so the key has no live version left
            None => {
                self.tree.delete(key)?;
            }
        }

        let mut count = 0;
        let mut rid = Some(dead);
        while let Some(doomed) = rid {
            let version = heap::get(self.tree.pool(), doomed)?;
            rid = version_prev(&version);
            heap::free(self.tree.pool(), doomed)?;
            count += 1;
        }

        Ok(count)
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
