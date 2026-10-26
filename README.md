<div align="center">
<pre>
 __  __  _   _  _____  __  __   ___   ____  __   __ _   _  _____ 
|  \/  || \ | || ____||  \/  | / _ \ / ___| \ \ / /| \ | || ____|
| |\/| ||  \| ||  _|  | |\/| || | | |\___ \  \ V / |  \| ||  _|  
| |  | || |\  || |___ | |  | || |_| | ___) |  | |  | |\  || |___ 
|_|  |_||_| \_||_____||_|  |_| \___/ |____/   |_|  |_| \_||_____|
</pre>

An embedded transactional storage engine in Rust, written from the disk up.

</div>

A key-value engine built one layer at a time, starting from raw bytes on disk and
ending at snapshot-isolated transactions. No SQL layer, no query planner. Just the
parts of a database that make data survive a power cut.

## Status

Everything below is built and tested. The engine is correct in process and
durable across a clean shutdown; every layer does its whole job, and none of
them is a stub waiting on the next one.

| Layer | What it does |
|-------|----------------|
| **Disk manager** | Positional 4 KB page I/O, allocate, `fsync` |
| **Pages** | Little-endian typed reads/writes; slotted layout for variable records |
| **Meta page** | Page 0, holding magic, version, the B+tree root, the heap tail and the next transaction timestamp |
| **Buffer pool** | Fixed frames, pin/unpin, LRU eviction, dirty flush |
| **B+tree** | Insert, lookup, delete, range scan, splits and merges |
| **Heap** | Append-only records on slotted pages; values live here and the tree points at them |
| **MVCC** | Per-key version chains, newest first, stamped with begin and end timestamps; snapshot visibility |
| **Db** | `begin`, `get`, `put`, `delete`, `abort`, `commit`, `gc` and `close` over all of the above |
| **Concurrency** | First writer wins: a write to a key a concurrent transaction already touched fails with `WriteConflict` |
| **Abort** | A transaction remembers the keys it wrote, undoes them and frees the versions it appended, leaving the ones underneath as they were |
| **GC** | A watermark below which nothing is reachable, then dead versions are freed and fully dead keys leave the tree |

Two limits in the tree are deliberate. A merged-away page is not reused yet, so
the file only grows; the free list comes with the space work below. And an
underfull node whose sibling will not fit is left underfull rather than
redistributed, which costs some density and no correctness.

The transaction layer gives snapshot isolation, not serializability. Dirty
reads, non-repeatable reads and lost updates are all prevented; write skew is
not, which is the anomaly snapshot isolation allows by definition. Durability
stops at a clean close: pages reach disk on eviction or `close`, so an unclean
crash can both lose committed work and bring back the writes of a transaction
that was still running, because `Db::open` starts with an empty active set and
no undo pass runs. `gc` frees a dead version's slot, but the bytes stay on the
page until a compaction pass and a page that empties out is not handed back to
the file.

Build notes live in [`docs/build-thread.md`](docs/build-thread.md).

## Still to come

Two segments are left. Neither of them goes back and changes a layer that is
already built.

**Durability.** A write-ahead log, then ARIES-style recovery. The log sits
beside the buffer pool and constrains when a page may be flushed, so a committed
transaction is on disk before the call returns. Recovery runs once at open:
redo what was logged and not applied, undo what was in flight. That closes the
crash hole described above.

**Space reuse.** Collecting a version frees its slot, not its bytes, and a page
that empties out stays allocated. Both want a space manager sitting under the
heap and the tree: in-page compaction to pack live records down, and a free
list of pages the file can hand back out. The B+tree needs the same free list
for the nodes it merges away, which is why this belongs below both of them
rather than inside GC.

## Layout

```
src/
  disk.rs           file-backed page I/O
  page.rs           Page, PageId, RecordId
  page/slotted.rs   slot directory, records grow from the end
  page/meta.rs      page 0
  buffer.rs         cache, pins, flush
  buffer/replacer.rs LRU
  btree/tree.rs     open, insert, lookup, delete, scan, splits, merges
  btree/node.rs     leaf / internal encoding and search
  heap.rs           append-only records, tail tracked in page 0
  mvcc/version.rs   version record: begin, end, prev, value
  mvcc/txn.rs       timestamps, snapshots, visibility, the gc watermark
  db.rs             transactional get / put over the tree and heap
tests/
  db.rs             end-to-end transaction scenarios
```

Pages are 4096 bytes. Slot entries are 4 bytes (`u16` offset + `u16` length). Leaves
store `RecordId` (page + slot) plus the key. Internal nodes store child page ids
and separators. Page 0 is reserved as the meta page, so `PageId(0)` is never a
tree node.

A value is stored in the heap as a version: a 26-byte header (`begin` and `end`
timestamps, then the `RecordId` of the previous version) followed by the value.
The leaf points at the newest version and each version points one step older, so
an update appends a version and rewrites 10 bytes in the leaf.

## Use

```rust
use mnemosyne::db::Db;

let mut db = Db::open(path, /* frame_count */ 32)?;

let mut txn = db.begin()?;
db.put(&mut txn, b"alice", b"100")?;
db.commit(txn);

let reader = db.begin()?;
let value = db.get(&reader, b"alice")?;

let mut txn = db.begin()?;
let existed = db.delete(&mut txn, b"alice")?;
db.commit(txn);

let mut txn = db.begin()?;
db.put(&mut txn, b"bob", b"200")?;
db.abort(txn)?;

let freed = db.gc()?;

db.close()?;
```

`open` creates a fresh file or reopens an existing one, and timestamps carry on
from where the last session stopped. A transaction sees its own writes and
everything committed before it began, and nothing else, for as long as it runs.
`commit` makes its writes visible to transactions that begin afterwards.
`delete` ends the newest version and reports whether the key was there; older
transactions keep reading the value. A `put` or `delete` on a key a concurrent
transaction already wrote returns `WriteConflict`, and the caller retries in a
new transaction. `abort` undoes everything a transaction wrote, which is what a
caller reaches for after a conflict. `gc` frees every version no transaction can
reach any more; nothing calls it on its own, and it is safe at any time,
including while transactions are open, because an open transaction holds the
watermark down. `close` writes every dirty page to disk.

The B+tree underneath is usable on its own (`BTree::open`, `insert`, `lookup`,
`scan`, `delete`), mapping keys to `RecordId`s. `scan` is half-open:
`[start, end)`.

## Build

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Warnings are errors. A pre-commit hook runs formatting, lints and tests before
anything lands. `./scripts/check.sh` is the same sequence as CI.

## Name

Two waters sat side by side in the underworld. Drink from Lethe and you forget
everything. Drink from Mnemosyne, the Greek titan of memory, and you keep it.

Every storage engine is trying to be the second one.
