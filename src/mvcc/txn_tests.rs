use super::*;

fn set(ids: &[u64]) -> BTreeSet<u64> {
    ids.iter().copied().collect()
}

#[test]
fn ids_start_where_the_manager_was_told() {
    let mut manager = TxnManager::new(42);

    assert_eq!(manager.begin().id, 42);
}

#[test]
fn ids_go_up_by_one() {
    let mut manager = TxnManager::new(1);

    let ids: Vec<u64> = (0..3).map(|_| manager.begin().id).collect();

    assert_eq!(ids, [1, 2, 3]);
}

#[test]
fn ids_are_never_reused_after_a_finish() {
    let mut manager = TxnManager::new(1);

    let first = manager.begin();
    manager.finish(first.id);

    assert_eq!(manager.begin().id, 2);
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

    assert!(!txn.running_at_begin.contains(&txn.id));
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

    manager.finish(first.id);

    assert_eq!(manager.begin().running_at_begin, set(&[]));
}

#[test]
fn finishing_removes_only_that_txn() {
    let mut manager = TxnManager::new(1);
    manager.begin();
    let second = manager.begin();
    manager.begin();

    manager.finish(second.id);

    assert_eq!(manager.begin().running_at_begin, set(&[1, 3]));
}

#[test]
fn a_snapshot_does_not_change_after_begin() {
    let mut manager = TxnManager::new(1);
    let first = manager.begin();
    let second = manager.begin();

    manager.finish(first.id);

    assert_eq!(second.running_at_begin, set(&[1]));
}

#[test]
fn a_commit_between_two_begins_shows_in_one_snapshot_and_not_the_next() {
    let mut manager = TxnManager::new(1);
    let t1 = manager.begin();
    let t2 = manager.begin();
    manager.finish(t1.id);
    let t3 = manager.begin();

    assert_eq!(t2.running_at_begin, set(&[1]));
    assert_eq!(t3.running_at_begin, set(&[2]));
}
