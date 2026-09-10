use mnemosyne::db::Db;
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
