use mnemosyne::db::Db;
use mnemosyne::error::Error;
use mnemosyne::mvcc::Txn;
use tempfile::TempDir;

const FRAMES: usize = 16;

fn temp_db() -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mnemosyne.db");
    (dir, path)
}

fn write_committed(db: &mut Db, key: &[u8], value: &[u8]) {
    let mut txn = db.begin().unwrap();
    db.put(&mut txn, key, value).unwrap();
    db.commit(txn);
}

fn balance(db: &mut Db, txn: &Txn, key: &[u8]) -> u64 {
    let bytes = db.get(txn, key).unwrap().unwrap();
    u64::from_le_bytes(bytes.try_into().unwrap())
}

#[test]
fn a_fresh_db_has_no_keys() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    let txn = db.begin().unwrap();

    assert_eq!(db.get(&txn, b"alice").unwrap(), None);
}

#[test]
fn a_txn_reads_its_own_write() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    let mut txn = db.begin().unwrap();
    db.put(&mut txn, b"alice", b"100").unwrap();

    assert_eq!(db.get(&txn, b"alice").unwrap(), Some(b"100".to_vec()));
}

#[test]
fn a_txn_rereading_its_own_overwrite_gets_the_latest() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    let mut txn = db.begin().unwrap();
    db.put(&mut txn, b"alice", b"100").unwrap();
    db.put(&mut txn, b"alice", b"200").unwrap();

    assert_eq!(db.get(&txn, b"alice").unwrap(), Some(b"200".to_vec()));
}

#[test]
fn a_committed_write_is_seen_by_a_later_txn() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    write_committed(&mut db, b"alice", b"100");
    let reader = db.begin().unwrap();

    assert_eq!(db.get(&reader, b"alice").unwrap(), Some(b"100".to_vec()));
}

#[test]
fn an_uncommitted_write_is_invisible_to_other_txns() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    let mut writer = db.begin().unwrap();
    db.put(&mut writer, b"alice", b"100").unwrap();
    let reader = db.begin().unwrap();

    assert_eq!(db.get(&reader, b"alice").unwrap(), None);
}

#[test]
fn a_write_committed_while_a_txn_runs_stays_invisible_to_it() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    let mut writer = db.begin().unwrap();
    db.put(&mut writer, b"alice", b"100").unwrap();
    let reader = db.begin().unwrap();
    db.commit(writer);

    assert_eq!(db.get(&reader, b"alice").unwrap(), None);
}

#[test]
fn a_txn_reads_the_same_value_twice_even_if_someone_commits_in_between() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let reader = db.begin().unwrap();
    let first = db.get(&reader, b"alice").unwrap();
    write_committed(&mut db, b"alice", b"200");
    let second = db.get(&reader, b"alice").unwrap();

    assert_eq!(first, Some(b"100".to_vec()));
    assert_eq!(second, first);
}

#[test]
fn an_overwrite_is_seen_by_txns_that_begin_after_it() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    write_committed(&mut db, b"alice", b"100");
    write_committed(&mut db, b"alice", b"200");
    let reader = db.begin().unwrap();

    assert_eq!(db.get(&reader, b"alice").unwrap(), Some(b"200".to_vec()));
}

#[test]
fn an_old_snapshot_walks_back_through_several_versions() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"1");

    let old = db.begin().unwrap();
    for value in [b"2", b"3", b"4"] {
        write_committed(&mut db, b"alice", value);
    }
    let new = db.begin().unwrap();

    assert_eq!(db.get(&old, b"alice").unwrap(), Some(b"1".to_vec()));
    assert_eq!(db.get(&new, b"alice").unwrap(), Some(b"4".to_vec()));
}

#[test]
fn the_bank_report_always_sums_to_1500() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    let mut setup = db.begin().unwrap();
    db.put(&mut setup, b"x", &1000u64.to_le_bytes()).unwrap();
    db.put(&mut setup, b"y", &500u64.to_le_bytes()).unwrap();
    db.commit(setup);

    let report = db.begin().unwrap();
    let x = balance(&mut db, &report, b"x");

    let mut transfer = db.begin().unwrap();
    db.put(&mut transfer, b"x", &900u64.to_le_bytes()).unwrap();
    db.put(&mut transfer, b"y", &600u64.to_le_bytes()).unwrap();
    db.commit(transfer);

    let y = balance(&mut db, &report, b"y");
    assert_eq!(x + y, 1500);

    let after = db.begin().unwrap();
    assert_eq!(balance(&mut db, &after, b"x"), 900);
    assert_eq!(balance(&mut db, &after, b"y"), 600);
}

#[test]
fn many_keys_all_read_back() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    for i in 0..500u32 {
        write_committed(&mut db, format!("key{i:04}").as_bytes(), &i.to_le_bytes());
    }
    let reader = db.begin().unwrap();

    for i in 0..500u32 {
        let value = db.get(&reader, format!("key{i:04}").as_bytes()).unwrap();
        assert_eq!(value, Some(i.to_le_bytes().to_vec()));
    }
}

#[test]
fn committed_data_survives_a_reopen() {
    let (_dir, path) = temp_db();
    {
        let mut db = Db::open(&path, FRAMES).unwrap();
        write_committed(&mut db, b"alice", b"100");
        db.close().unwrap();
    }

    let mut db = Db::open(&path, FRAMES).unwrap();
    let reader = db.begin().unwrap();

    assert_eq!(db.get(&reader, b"alice").unwrap(), Some(b"100".to_vec()));
}

#[test]
fn timestamps_carry_on_after_a_reopen() {
    let (_dir, path) = temp_db();
    {
        let mut db = Db::open(&path, FRAMES).unwrap();
        for _ in 0..3 {
            let txn = db.begin().unwrap();
            db.commit(txn);
        }
        write_committed(&mut db, b"alice", b"100");
        db.close().unwrap();
    }

    let mut db = Db::open(&path, FRAMES).unwrap();
    let reader = db.begin().unwrap();

    assert_eq!(db.get(&reader, b"alice").unwrap(), Some(b"100".to_vec()));
}

#[test]
fn the_second_writer_of_a_key_gets_a_conflict() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut first = db.begin().unwrap();
    let mut second = db.begin().unwrap();
    db.put(&mut first, b"alice", b"150").unwrap();

    assert!(matches!(
        db.put(&mut second, b"alice", b"130"),
        Err(Error::WriteConflict)
    ));
}

#[test]
fn the_older_txn_loses_if_a_younger_one_wrote_first() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut older = db.begin().unwrap();
    let mut younger = db.begin().unwrap();
    db.put(&mut younger, b"alice", b"130").unwrap();

    assert!(matches!(
        db.put(&mut older, b"alice", b"150"),
        Err(Error::WriteConflict)
    ));
}

#[test]
fn a_write_committed_after_a_txn_began_still_conflicts() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut late = db.begin().unwrap();
    write_committed(&mut db, b"alice", b"200");

    assert!(matches!(
        db.put(&mut late, b"alice", b"150"),
        Err(Error::WriteConflict)
    ));
}

#[test]
fn a_write_committed_before_a_txn_began_does_not_conflict() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut txn = db.begin().unwrap();

    assert!(db.put(&mut txn, b"alice", b"150").is_ok());
}

#[test]
fn writes_to_different_keys_never_conflict() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    let mut first = db.begin().unwrap();
    let mut second = db.begin().unwrap();

    assert!(db.put(&mut first, b"alice", b"100").is_ok());
    assert!(db.put(&mut second, b"bob", b"200").is_ok());
}

#[test]
fn a_rejected_put_leaves_the_key_untouched() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut winner = db.begin().unwrap();
    let mut loser = db.begin().unwrap();
    db.put(&mut winner, b"alice", b"150").unwrap();
    db.put(&mut loser, b"alice", b"130").unwrap_err();
    db.commit(winner);
    let reader = db.begin().unwrap();

    assert_eq!(db.get(&loser, b"alice").unwrap(), Some(b"100".to_vec()));
    assert_eq!(db.get(&reader, b"alice").unwrap(), Some(b"150".to_vec()));
}

#[test]
fn concurrent_increments_lose_nothing_when_the_loser_retries() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", &100u64.to_le_bytes());

    let mut plus_50 = db.begin().unwrap();
    let mut plus_30 = db.begin().unwrap();
    let seen_by_50 = balance(&mut db, &plus_50, b"alice");
    let seen_by_30 = balance(&mut db, &plus_30, b"alice");
    db.put(&mut plus_50, b"alice", &(seen_by_50 + 50).to_le_bytes())
        .unwrap();
    let rejected = db.put(&mut plus_30, b"alice", &(seen_by_30 + 30).to_le_bytes());
    db.commit(plus_50);
    db.commit(plus_30);

    let mut retry = db.begin().unwrap();
    let seen_by_retry = balance(&mut db, &retry, b"alice");
    db.put(&mut retry, b"alice", &(seen_by_retry + 30).to_le_bytes())
        .unwrap();
    db.commit(retry);
    let reader = db.begin().unwrap();

    assert!(matches!(rejected, Err(Error::WriteConflict)));
    assert_eq!(balance(&mut db, &reader, b"alice"), 180);
}

#[test]
fn deleting_a_key_that_was_never_written_is_false() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    let mut txn = db.begin().unwrap();

    assert!(!db.delete(&mut txn, b"alice").unwrap());
}

#[test]
fn a_deleted_key_is_gone_for_txns_that_begin_after() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut deleter = db.begin().unwrap();
    assert!(db.delete(&mut deleter, b"alice").unwrap());
    db.commit(deleter);
    let reader = db.begin().unwrap();

    assert_eq!(db.get(&reader, b"alice").unwrap(), None);
}

#[test]
fn a_txn_no_longer_sees_a_key_it_deleted() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut txn = db.begin().unwrap();
    db.delete(&mut txn, b"alice").unwrap();

    assert_eq!(db.get(&txn, b"alice").unwrap(), None);
}

#[test]
fn an_uncommitted_delete_is_invisible_to_other_txns() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut deleter = db.begin().unwrap();
    db.delete(&mut deleter, b"alice").unwrap();
    let reader = db.begin().unwrap();

    assert_eq!(db.get(&reader, b"alice").unwrap(), Some(b"100".to_vec()));
}

#[test]
fn a_txn_that_began_before_a_delete_still_reads_the_value() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let reader = db.begin().unwrap();
    let mut deleter = db.begin().unwrap();
    db.delete(&mut deleter, b"alice").unwrap();
    db.commit(deleter);

    assert_eq!(db.get(&reader, b"alice").unwrap(), Some(b"100".to_vec()));
}

#[test]
fn deleting_the_same_key_twice_reports_it_once() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut deleter = db.begin().unwrap();
    let first = db.delete(&mut deleter, b"alice").unwrap();
    let second = db.delete(&mut deleter, b"alice").unwrap();
    db.commit(deleter);
    let mut later = db.begin().unwrap();
    let third = db.delete(&mut later, b"alice").unwrap();

    assert_eq!((first, second, third), (true, false, false));
}

#[test]
fn a_deleted_key_can_be_written_again() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut deleter = db.begin().unwrap();
    db.delete(&mut deleter, b"alice").unwrap();
    db.commit(deleter);
    write_committed(&mut db, b"alice", b"200");
    let reader = db.begin().unwrap();

    assert_eq!(db.get(&reader, b"alice").unwrap(), Some(b"200".to_vec()));
}

#[test]
fn writing_a_deleted_key_again_does_not_bring_it_back_for_txns_in_between() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut deleter = db.begin().unwrap();
    db.delete(&mut deleter, b"alice").unwrap();
    db.commit(deleter);
    let in_between = db.begin().unwrap();
    write_committed(&mut db, b"alice", b"200");
    let after = db.begin().unwrap();

    assert_eq!(db.get(&in_between, b"alice").unwrap(), None);
    assert_eq!(db.get(&after, b"alice").unwrap(), Some(b"200".to_vec()));
}

#[test]
fn a_delete_conflicts_with_a_concurrent_put() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut writer = db.begin().unwrap();
    let mut deleter = db.begin().unwrap();
    db.put(&mut writer, b"alice", b"150").unwrap();

    assert!(matches!(
        db.delete(&mut deleter, b"alice"),
        Err(Error::WriteConflict)
    ));
}

#[test]
fn a_put_conflicts_with_a_concurrent_delete() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut deleter = db.begin().unwrap();
    let mut writer = db.begin().unwrap();
    db.delete(&mut deleter, b"alice").unwrap();

    assert!(matches!(
        db.put(&mut writer, b"alice", b"150"),
        Err(Error::WriteConflict)
    ));
}

#[test]
fn two_concurrent_deletes_conflict_instead_of_both_succeeding() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");

    let mut first = db.begin().unwrap();
    let mut second = db.begin().unwrap();
    db.delete(&mut first, b"alice").unwrap();

    assert!(matches!(
        db.delete(&mut second, b"alice"),
        Err(Error::WriteConflict)
    ));
}

#[test]
fn deleting_a_key_leaves_other_keys_alone() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();
    write_committed(&mut db, b"alice", b"100");
    write_committed(&mut db, b"bob", b"200");

    let mut deleter = db.begin().unwrap();
    db.delete(&mut deleter, b"alice").unwrap();
    db.commit(deleter);
    let reader = db.begin().unwrap();

    assert_eq!(db.get(&reader, b"bob").unwrap(), Some(b"200".to_vec()));
}

#[test]
fn a_delete_survives_a_reopen() {
    let (_dir, path) = temp_db();
    {
        let mut db = Db::open(&path, FRAMES).unwrap();
        write_committed(&mut db, b"alice", b"100");
        let mut deleter = db.begin().unwrap();
        db.delete(&mut deleter, b"alice").unwrap();
        db.commit(deleter);
        db.close().unwrap();
    }

    let mut db = Db::open(&path, FRAMES).unwrap();
    let reader = db.begin().unwrap();

    assert_eq!(db.get(&reader, b"alice").unwrap(), None);
}
