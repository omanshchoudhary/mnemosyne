use super::*;
use tempfile::TempDir;

const FRAMES: usize = 8;

// the TempDir has to outlive the tree, or the file is deleted underneath it
fn temp_db() -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tree.db");
    (dir, path)
}

fn rid(page: u64, slot: u16) -> RecordId {
    RecordId {
        page: PageId(page),
        slot,
    }
}

// how many entries the root leaf is holding
fn root_slot_count(tree: &mut BTree) -> u16 {
    let root = tree.root().unwrap();
    let frame = tree.pool.fetch_page(root).unwrap();
    let count = tree.pool.page(frame).slot_count();
    tree.pool.unpin(frame).unwrap();
    count
}

#[test]
fn a_fresh_file_gets_a_meta_page_and_an_empty_root_leaf() {
    let (_dir, path) = temp_db();
    let mut tree = BTree::open(&path, FRAMES).unwrap();

    // meta is page 0, so the first page the tree can use is 1
    assert_eq!(tree.root().unwrap(), PageId(1));

    let frame = tree.pool.fetch_page(PageId(1)).unwrap();
    let root = tree.pool.page(frame);
    assert!(root.is_leaf());
    assert_eq!(root.slot_count(), 0);
    assert_eq!(root.next_leaf(), None);
    tree.pool.unpin(frame).unwrap();
}

#[test]
fn opening_a_fresh_file_writes_both_pages_to_disk() {
    // open flushes, so the two pages must be on disk before it returns
    let (_dir, path) = temp_db();
    {
        BTree::open(&path, FRAMES).unwrap();
    }

    let len = std::fs::metadata(&path).unwrap().len();
    assert_eq!(len, 2 * crate::page::PAGE_SIZE as u64);
}

#[test]
fn reopening_finds_the_same_root() {
    let (_dir, path) = temp_db();
    {
        let mut tree = BTree::open(&path, FRAMES).unwrap();
        assert_eq!(tree.root().unwrap(), PageId(1));
    }

    // the second open must take the existing path, not bootstrap again
    let mut tree = BTree::open(&path, FRAMES).unwrap();
    assert_eq!(tree.root().unwrap(), PageId(1));
    assert_eq!(tree.pool.page_count().unwrap(), 2);
}

#[test]
fn a_new_root_survives_a_reopen() {
    // what a root split does: point the meta page somewhere else
    let (_dir, path) = temp_db();
    {
        let mut tree = BTree::open(&path, FRAMES).unwrap();
        tree.set_root(PageId(7)).unwrap();
        tree.pool.flush_all().unwrap();
    }

    let mut tree = BTree::open(&path, FRAMES).unwrap();
    assert_eq!(tree.root().unwrap(), PageId(7));
}

#[test]
fn a_file_that_is_not_a_database_is_rejected() {
    let (_dir, path) = temp_db();
    // one page of junk, so page_count is not 0 and the magic check runs
    std::fs::write(&path, vec![0xABu8; crate::page::PAGE_SIZE]).unwrap();

    assert!(matches!(
        BTree::open(&path, FRAMES),
        Err(Error::BadMetaPage)
    ));
}

#[test]
fn open_leaves_no_frame_pinned() {
    // a pin leak would only show up much later, as a full pool
    let (_dir, path) = temp_db();
    let mut tree = BTree::open(&path, 2).unwrap();

    // both frames hold open's pages, so allocating two more forces an
    // eviction each, which is only possible if open unpinned them
    for _ in 0..2 {
        let (_, frame) = tree.pool.new_page().unwrap();
        tree.pool.unpin(frame).unwrap();
    }
}

#[test]
fn a_key_comes_back_after_being_inserted() {
    let (_dir, path) = temp_db();
    let mut tree = BTree::open(&path, FRAMES).unwrap();

    tree.insert(b"apple", rid(57, 3)).unwrap();

    assert_eq!(tree.lookup(b"apple").unwrap(), Some(rid(57, 3)));
}

#[test]
fn a_key_that_was_never_inserted_is_none() {
    let (_dir, path) = temp_db();
    let mut tree = BTree::open(&path, FRAMES).unwrap();

    // empty tree, and a tree holding a neighbour on either side
    assert_eq!(tree.lookup(b"nothing").unwrap(), None);

    tree.insert(b"b", rid(1, 0)).unwrap();
    assert_eq!(tree.lookup(b"a").unwrap(), None);
    assert_eq!(tree.lookup(b"c").unwrap(), None);
}

#[test]
fn keys_inserted_out_of_order_all_come_back() {
    // the leaf must end up sorted however they arrive, or search misses
    let (_dir, path) = temp_db();
    let mut tree = BTree::open(&path, FRAMES).unwrap();

    let keys: [&[u8]; 7] = [b"m", b"c", b"t", b"a", b"z", b"f", b"q"];
    for (i, key) in keys.iter().enumerate() {
        tree.insert(key, rid(9, i as u16)).unwrap();
    }

    for (i, key) in keys.iter().enumerate() {
        assert_eq!(tree.lookup(key).unwrap(), Some(rid(9, i as u16)));
    }
    assert_eq!(root_slot_count(&mut tree), 7);
}

#[test]
fn inserting_the_same_key_twice_overwrites_it() {
    let (_dir, path) = temp_db();
    let mut tree = BTree::open(&path, FRAMES).unwrap();

    tree.insert(b"key", rid(1, 1)).unwrap();
    tree.insert(b"key", rid(2, 2)).unwrap();

    assert_eq!(tree.lookup(b"key").unwrap(), Some(rid(2, 2)));
    // an overwrite is an edit, not a second entry
    assert_eq!(root_slot_count(&mut tree), 1);
}

#[test]
fn an_overwrite_leaves_its_neighbours_alone() {
    // the rewrite is 10 bytes inside one entry, so nothing else can move
    let (_dir, path) = temp_db();
    let mut tree = BTree::open(&path, FRAMES).unwrap();

    tree.insert(b"a", rid(1, 0)).unwrap();
    tree.insert(b"b", rid(2, 0)).unwrap();
    tree.insert(b"c", rid(3, 0)).unwrap();

    tree.insert(b"b", rid(99, 9)).unwrap();

    assert_eq!(tree.lookup(b"a").unwrap(), Some(rid(1, 0)));
    assert_eq!(tree.lookup(b"b").unwrap(), Some(rid(99, 9)));
    assert_eq!(tree.lookup(b"c").unwrap(), Some(rid(3, 0)));
    assert_eq!(root_slot_count(&mut tree), 3);
}

#[test]
fn inserted_keys_survive_a_reopen() {
    let (_dir, path) = temp_db();
    {
        let mut tree = BTree::open(&path, FRAMES).unwrap();
        tree.insert(b"persisted", rid(4, 2)).unwrap();
        tree.pool.flush_all().unwrap();
    }

    let mut tree = BTree::open(&path, FRAMES).unwrap();
    assert_eq!(tree.lookup(b"persisted").unwrap(), Some(rid(4, 2)));
}

#[test]
fn insert_leaves_no_frame_pinned() {
    let (_dir, path) = temp_db();
    let mut tree = BTree::open(&path, 3).unwrap();

    // more inserts than frames, so a leaked pin per insert fills the pool
    for i in 0..20u16 {
        tree.insert(format!("key{i:03}").as_bytes(), rid(1, i))
            .unwrap();
    }

    assert_eq!(tree.lookup(b"key019").unwrap(), Some(rid(1, 19)));
}

// the leaf that holds the smallest keys, found by descending on an empty key
fn leftmost_leaf(tree: &mut BTree) -> PageId {
    let mut page_id = tree.root().unwrap();
    loop {
        let frame = tree.pool.fetch_page(page_id).unwrap();
        let page = tree.pool.page(frame);
        if page.is_leaf() {
            tree.pool.unpin(frame).unwrap();
            return page_id;
        }
        let child = page.child_for_key(b"").unwrap();
        tree.pool.unpin(frame).unwrap();
        page_id = child;
    }
}

fn key_of(i: u16) -> Vec<u8> {
    format!("{i:04}").into_bytes()
}

#[test]
fn the_tree_grows_past_a_single_page() {
    let (_dir, path) = temp_db();
    let mut tree = BTree::open(&path, FRAMES).unwrap();

    for i in 0..1000u16 {
        tree.insert(&key_of(i), rid(1, i)).unwrap();
    }

    // the root started life as a leaf, so an internal root proves it split
    let root = tree.root().unwrap();
    let frame = tree.pool.fetch_page(root).unwrap();
    assert!(tree.pool.page(frame).is_internal());
    tree.pool.unpin(frame).unwrap();

    assert!(root != PageId(1), "the root pointer never moved");
    assert!(tree.pool.page_count().unwrap() > 2);

    for i in 0..1000u16 {
        assert_eq!(tree.lookup(&key_of(i)).unwrap(), Some(rid(1, i)));
    }
}

#[test]
fn scrambled_inserts_are_all_findable_after_splits() {
    // ascending keys always split the same way, this exercises the others
    let (_dir, path) = temp_db();
    let mut tree = BTree::open(&path, FRAMES).unwrap();

    let mut order: Vec<u16> = (0..800).collect();
    let mut seed = 0x2545_F491u32;
    for i in (1..order.len()).rev() {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        order.swap(i, seed as usize % (i + 1));
    }

    for &i in &order {
        tree.insert(&key_of(i), rid(1, i)).unwrap();
    }

    for i in 0..800u16 {
        assert_eq!(tree.lookup(&key_of(i)).unwrap(), Some(rid(1, i)));
    }
    assert_eq!(tree.lookup(b"9999").unwrap(), None);
}

#[test]
fn the_leaf_chain_holds_every_key_in_order() {
    // walking next_leaf must visit every key exactly once, in sorted order
    let (_dir, path) = temp_db();
    let mut tree = BTree::open(&path, FRAMES).unwrap();

    for i in 0..600u16 {
        tree.insert(&key_of(i), rid(1, i)).unwrap();
    }

    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut leaf = Some(leftmost_leaf(&mut tree));
    let mut leaves = 0;

    while let Some(page_id) = leaf {
        let frame = tree.pool.fetch_page(page_id).unwrap();
        let page = tree.pool.page(frame);
        for slot in 0..page.slot_count() {
            seen.push(page.leaf_key(slot).unwrap().to_vec());
        }
        leaf = page.next_leaf();
        tree.pool.unpin(frame).unwrap();
        leaves += 1;
    }

    assert!(leaves > 1, "expected several leaves, got {leaves}");
    assert_eq!(seen.len(), 600);
    let expected: Vec<Vec<u8>> = (0..600u16).map(key_of).collect();
    assert_eq!(seen, expected);
}

#[test]
fn a_split_tree_survives_a_reopen() {
    let (_dir, path) = temp_db();
    {
        let mut tree = BTree::open(&path, FRAMES).unwrap();
        for i in 0..500u16 {
            tree.insert(&key_of(i), rid(1, i)).unwrap();
        }
        tree.pool.flush_all().unwrap();
    }

    let mut tree = BTree::open(&path, FRAMES).unwrap();
    for i in 0..500u16 {
        assert_eq!(tree.lookup(&key_of(i)).unwrap(), Some(rid(1, i)));
    }
}

#[test]
fn overwriting_after_a_split_finds_the_right_leaf() {
    let (_dir, path) = temp_db();
    let mut tree = BTree::open(&path, FRAMES).unwrap();

    for i in 0..500u16 {
        tree.insert(&key_of(i), rid(1, i)).unwrap();
    }
    // a key in the middle, so the descent has to pick a child correctly
    tree.insert(&key_of(250), rid(99, 9)).unwrap();

    assert_eq!(tree.lookup(&key_of(250)).unwrap(), Some(rid(99, 9)));
    assert_eq!(tree.lookup(&key_of(249)).unwrap(), Some(rid(1, 249)));
    assert_eq!(tree.lookup(&key_of(251)).unwrap(), Some(rid(1, 251)));
}
