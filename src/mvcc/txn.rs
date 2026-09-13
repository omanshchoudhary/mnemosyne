#![allow(dead_code)]

use std::collections::BTreeSet;

use crate::mvcc::version::NO_END;

pub struct Txn {
    pub(crate) timestamp: u64,
    // snapshot: other transactions that had begun but hadn't committed or aborted at the instant I started
    pub(crate) running_at_begin: BTreeSet<u64>,
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
}

impl TxnManager {
    pub(crate) fn new(next_timestamp: u64) -> Self {
        Self {
            next_timestamp,
            active: BTreeSet::new(),
        }
    }

    // configure a new Txn beginning
    pub(crate) fn begin(&mut self) -> Txn {
        let timestamp = self.next_timestamp;
        self.next_timestamp += 1;
        let running_at_begin = self.active.clone();
        self.active.insert(timestamp);
        Txn {
            timestamp,
            running_at_begin,
        }
    }

    pub(crate) fn finish(&mut self, timestamp: u64) {
        self.active.remove(&timestamp);
    }
}

#[cfg(test)]
#[path = "txn_tests.rs"]
mod tests;
