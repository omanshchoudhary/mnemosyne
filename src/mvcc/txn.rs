#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use crate::mvcc::version::NO_END;

pub struct Txn {
    pub(crate) timestamp: u64,
    // snapshot: other transactions that had begun but hadn't committed or aborted at the instant I started
    pub(crate) running_at_begin: BTreeSet<u64>,

    // keys i wrote, the undo list for abort
    pub(crate) writes: Vec<Vec<u8>>,
}

impl Txn {
    // can i see writer_txn work
    pub(crate) fn sees(&self, writer_txn: u64) -> bool {
        self.timestamp == writer_txn
            || (self.timestamp > writer_txn && !self.running_at_begin.contains(&writer_txn))
    }

    // can i see this version of record
    pub(crate) fn sees_version(&self, begin: u64, end: u64) -> bool {
        self.sees(begin) && !self.sees(end)
    }

    pub(crate) fn can_add_newer_version(&self, begin: u64, end: u64) -> bool {
        let last_writer = if end == NO_END { begin } else { end };
        self.sees(last_writer)
    }
}

pub(crate) struct TxnManager {
    next_timestamp: u64,
    // current running transactions
    active: BTreeSet<u64>,
    // per running txn, the oldest timestamp it still has to be able to read
    floors: BTreeMap<u64, u64>,
}

impl TxnManager {
    pub(crate) fn new(next_timestamp: u64) -> Self {
        Self {
            next_timestamp,
            active: BTreeSet::new(),
            floors: BTreeMap::new(),
        }
    }

    // configure a new Txn beginning
    pub(crate) fn begin(&mut self) -> Txn {
        let timestamp = self.next_timestamp;
        self.next_timestamp += 1;
        let running_at_begin = self.active.clone();
        let floor = running_at_begin.first().copied().unwrap_or(timestamp);
        self.floors.insert(timestamp, floor);
        self.active.insert(timestamp);
        Txn {
            timestamp,
            running_at_begin,
            writes: Vec::new(),
        }
    }

    // nothing below this can ever be read again
    pub(crate) fn watermark(&self) -> u64 {
        self.floors
            .values()
            .min()
            .copied()
            // nothing running, so every replaced version is free to go
            .unwrap_or(self.next_timestamp)
    }

    pub(crate) fn finish(&mut self, timestamp: u64) {
        self.active.remove(&timestamp);
        self.floors.remove(&timestamp);
    }
}

#[cfg(test)]
#[path = "txn_tests.rs"]
mod tests;
