use super::*;
use crate::mvcc::version::NO_END;

fn set(timestamps: &[u64]) -> BTreeSet<u64> {
    timestamps.iter().copied().collect()
}

fn txn(timestamp: u64, running_at_begin: &[u64]) -> Txn {
    Txn {
        timestamp,
        running_at_begin: set(running_at_begin),
    }
}

fn visible(txn: &Txn, chain: &[(u64, u64)]) -> Vec<(u64, u64)> {
    chain
        .iter()
        .copied()
        .filter(|&(begin, end)| txn.sees_version(begin, end))
        .collect()
}

#[test]
fn timestamps_start_where_the_manager_was_told() {
    let mut manager = TxnManager::new(42);

    assert_eq!(manager.begin().timestamp, 42);
}

#[test]
fn timestamps_go_up_by_one() {
    let mut manager = TxnManager::new(1);

    let timestamps: Vec<u64> = (0..3).map(|_| manager.begin().timestamp).collect();

    assert_eq!(timestamps, [1, 2, 3]);
}

#[test]
fn timestamps_are_never_reused_after_a_finish() {
    let mut manager = TxnManager::new(1);

    let first = manager.begin();
    manager.finish(first.timestamp);

    assert_eq!(manager.begin().timestamp, 2);
}

#[test]
fn the_first_txn_sees_no_one_running() {
    let mut manager = TxnManager::new(1);

    assert_eq!(manager.begin().running_at_begin, set(&[]));
}

#[test]
fn a_txn_is_not_in_its_own_snapshot() {
    let mut manager = TxnManager::new(1);
    manager.begin();

    let txn = manager.begin();

    assert!(!txn.running_at_begin.contains(&txn.timestamp));
}

#[test]
fn a_txn_sees_everyone_still_running_when_it_began() {
    let mut manager = TxnManager::new(1);
    manager.begin();
    manager.begin();

    assert_eq!(manager.begin().running_at_begin, set(&[1, 2]));
}

#[test]
fn a_finished_txn_is_not_in_later_snapshots() {
    let mut manager = TxnManager::new(1);
    let first = manager.begin();

    manager.finish(first.timestamp);

    assert_eq!(manager.begin().running_at_begin, set(&[]));
}

#[test]
fn finishing_removes_only_that_txn() {
    let mut manager = TxnManager::new(1);
    manager.begin();
    let second = manager.begin();
    manager.begin();

    manager.finish(second.timestamp);

    assert_eq!(manager.begin().running_at_begin, set(&[1, 3]));
}

#[test]
fn a_snapshot_does_not_change_after_begin() {
    let mut manager = TxnManager::new(1);
    let first = manager.begin();
    let second = manager.begin();

    manager.finish(first.timestamp);

    assert_eq!(second.running_at_begin, set(&[1]));
}

#[test]
fn a_commit_between_two_begins_shows_in_one_snapshot_and_not_the_next() {
    let mut manager = TxnManager::new(1);
    let t1 = manager.begin();
    let t2 = manager.begin();
    manager.finish(t1.timestamp);
    let t3 = manager.begin();

    assert_eq!(t2.running_at_begin, set(&[1]));
    assert_eq!(t3.running_at_begin, set(&[2]));
}

#[test]
fn a_txn_sees_its_own_writes() {
    assert!(txn(5, &[]).sees(5));
}

#[test]
fn a_txn_sees_an_older_writer_that_had_finished() {
    assert!(txn(5, &[]).sees(3));
}

#[test]
fn a_txn_does_not_see_a_younger_writer() {
    assert!(!txn(5, &[]).sees(7));
}

#[test]
fn a_txn_does_not_see_an_older_writer_still_running_when_it_began() {
    assert!(!txn(5, &[3]).sees(3));
}

#[test]
fn no_txn_ever_sees_no_end() {
    assert!(!txn(u64::MAX - 1, &[]).sees(NO_END));
}

#[test]
fn a_txn_from_before_an_update_reads_the_old_version() {
    let chain = [(7, NO_END), (3, 7)];

    assert_eq!(visible(&txn(5, &[]), &chain), [(3, 7)]);
}

#[test]
fn a_txn_from_after_an_update_reads_the_new_version() {
    let chain = [(7, NO_END), (3, 7)];

    assert_eq!(visible(&txn(9, &[]), &chain), [(7, NO_END)]);
}

#[test]
fn a_txn_that_began_while_the_update_ran_reads_the_old_version() {
    let chain = [(7, NO_END), (3, 7)];

    assert_eq!(visible(&txn(9, &[7]), &chain), [(3, 7)]);
}

#[test]
fn the_updater_reads_its_own_new_version() {
    let chain = [(7, NO_END), (3, 7)];

    assert_eq!(visible(&txn(7, &[]), &chain), [(7, NO_END)]);
}

#[test]
fn txn_4_with_3_running_reads_the_oldest_version() {
    let chain = [(7, NO_END), (3, 7), (1, 3)];

    assert_eq!(visible(&txn(4, &[3]), &chain), [(1, 3)]);
}

#[test]
fn a_txn_older_than_every_version_reads_nothing() {
    let chain = [(7, NO_END), (3, 7)];

    assert_eq!(visible(&txn(2, &[]), &chain), []);
}

#[test]
fn a_version_ended_with_no_replacement_is_gone_for_later_txns() {
    let chain = [(3, 7)];

    assert_eq!(visible(&txn(5, &[]), &chain), [(3, 7)]);
    assert_eq!(visible(&txn(9, &[]), &chain), []);
}

#[test]
fn every_later_txn_reads_exactly_one_version() {
    let chain = [(7, NO_END), (3, 7), (1, 3)];

    for timestamp in 1..=20 {
        assert_eq!(visible(&txn(timestamp, &[]), &chain).len(), 1);
    }
}

#[test]
fn a_txn_can_add_on_top_of_a_version_committed_before_it_began() {
    assert!(txn(5, &[]).can_add_newer_version(3, NO_END));
}

#[test]
fn a_txn_can_add_on_top_of_its_own_version() {
    assert!(txn(5, &[]).can_add_newer_version(5, NO_END));
}

#[test]
fn a_txn_cannot_add_on_top_of_a_version_from_a_txn_running_when_it_began() {
    assert!(!txn(5, &[3]).can_add_newer_version(3, NO_END));
}

#[test]
fn a_txn_cannot_add_on_top_of_a_version_from_a_younger_txn() {
    assert!(!txn(5, &[]).can_add_newer_version(7, NO_END));
}

#[test]
fn an_ended_head_is_judged_by_the_txn_that_ended_it() {
    assert!(txn(9, &[]).can_add_newer_version(3, 7));
    assert!(!txn(9, &[7]).can_add_newer_version(3, 7));
}
