#![allow(dead_code)]

use std::collections::BTreeSet;

pub struct Txn {
    pub(crate) id: u64,
    // snapshot: other transactions that had begun but hadn't committed or aborted at the instant I started
    pub(crate) running_at_begin: BTreeSet<u64>,
}

pub(crate) struct TxnManager {
    next_id: u64,
    // current running transactions
    active: BTreeSet<u64>,
}

impl TxnManager {
    pub(crate) fn new(next_id: u64) -> Self {
        Self {
            next_id,
            active: BTreeSet::new(),
        }
    }

    pub(crate) fn begin(&mut self) -> Txn {
        let id = self.next_id;
        self.next_id += 1;
        let running_at_begin = self.active.clone();
        self.active.insert(id);
        Txn {
            id,
            running_at_begin,
        }
    }

    pub(crate) fn finish(&mut self, id: u64) {
        self.active.remove(&id);
    }
}

#[cfg(test)]
#[path = "txn_tests.rs"]
mod tests;
