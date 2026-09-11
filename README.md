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

Work in progress. Storage, the index and a first cut of MVCC are in:

| Layer | What it does |
|-------|----------------|
| **Disk manager** | Positional 4 KB page I/O, allocate, `fsync` |
| **Pages** | Little-endian typed reads/writes; slotted layout for variable records |
| **Meta page** | Page 0, holding magic, version, the B+tree root, the heap tail and the next transaction timestamp |
| **Buffer pool** | Fixed frames, pin/unpin, LRU eviction, dirty flush |
| **B+tree** | Insert, lookup, delete, range scan, splits and merges |
| **Heap** | Append-only records on slotted pages; values live here and the tree points at them |
| **MVCC** | Per-key version chains, newest first, stamped with begin and end timestamps; snapshot visibility |
| **Db** | `begin`, `get`, `put`, `commit` and `close` over all of the above |

Still ahead: write-write conflict detection, abort, delete through `Db`, version
GC, WAL, ARIES-style recovery.

Two limits in the tree are deliberate. A merged-away page is not reused yet, so
the file only grows; the free list belongs with GC. And an underfull node whose
sibling will not fit is left underfull rather than redistributed, which costs
some density and no correctness.

The transaction layer is a first cut. Reads are snapshot-consistent, but until
conflict detection lands, two transactions can both update the same key and one
of the writes is lost. There is no abort yet. A commit is not durable on its
own: pages reach disk on eviction or `close`, so a crash can lose committed work
until the WAL is in. Old versions are never reclaimed.

Build notes live in [`docs/build-thread.md`](docs/build-thread.md).

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
  mvcc/txn.rs       timestamps, snapshots, visibility
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

db.close()?;
```

`open` creates a fresh file or reopens an existing one, and timestamps carry on
from where the last session stopped. A transaction sees its own writes and
everything committed before it began, and nothing else, for as long as it runs.
`commit` makes its writes visible to transactions that begin afterwards. `close`
writes every dirty page to disk.

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
