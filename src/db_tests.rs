use super::*;
use tempfile::TempDir;

const FRAMES: usize = 16;

fn temp_db() -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.db");
    (dir, path)
}

fn freed(db: &mut Db, rid: RecordId) -> bool {
    matches!(heap::get(db.tree.pool(), rid), Err(Error::SlotDeleted(_)))
}

#[test]
fn abort_frees_the_version_it_wrote() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    let mut first = db.begin().unwrap();
    db.put(&mut first, b"alice", b"100").unwrap();
    db.commit(first);

    let mut second = db.begin().unwrap();
    db.put(&mut second, b"alice", b"200").unwrap();
    let doomed = db.tree.lookup(b"alice").unwrap().unwrap();
    db.abort(second).unwrap();

    assert!(freed(&mut db, doomed));
}

#[test]
fn abort_frees_the_version_of_a_key_that_never_existed() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    let mut txn = db.begin().unwrap();
    db.put(&mut txn, b"ghost", b"boo").unwrap();
    let doomed = db.tree.lookup(b"ghost").unwrap().unwrap();
    db.abort(txn).unwrap();

    assert!(freed(&mut db, doomed));
}

#[test]
fn abort_frees_every_version_it_wrote_for_one_key() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    let mut first = db.begin().unwrap();
    db.put(&mut first, b"alice", b"100").unwrap();
    db.commit(first);

    let mut second = db.begin().unwrap();
    db.put(&mut second, b"alice", b"200").unwrap();
    let older = db.tree.lookup(b"alice").unwrap().unwrap();
    db.put(&mut second, b"alice", b"300").unwrap();
    let newer = db.tree.lookup(b"alice").unwrap().unwrap();
    db.abort(second).unwrap();

    assert!(freed(&mut db, older));
    assert!(freed(&mut db, newer));
}

#[test]
fn abort_leaves_the_surviving_version_alone() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    let mut first = db.begin().unwrap();
    db.put(&mut first, b"alice", b"100").unwrap();
    db.commit(first);
    let survivor = db.tree.lookup(b"alice").unwrap().unwrap();

    let mut second = db.begin().unwrap();
    db.put(&mut second, b"alice", b"200").unwrap();
    db.abort(second).unwrap();

    assert!(!freed(&mut db, survivor));
    let reader = db.begin().unwrap();
    assert_eq!(db.get(&reader, b"alice").unwrap(), Some(b"100".to_vec()));
}

#[test]
fn abort_frees_versions_across_every_key_it_touched() {
    let (_dir, path) = temp_db();
    let mut db = Db::open(&path, FRAMES).unwrap();

    let mut txn = db.begin().unwrap();
    db.put(&mut txn, b"alice", b"100").unwrap();
    db.put(&mut txn, b"bob", b"200").unwrap();
    let alice = db.tree.lookup(b"alice").unwrap().unwrap();
    let bob = db.tree.lookup(b"bob").unwrap().unwrap();
    db.abort(txn).unwrap();

    assert!(freed(&mut db, alice));
    assert!(freed(&mut db, bob));
}
