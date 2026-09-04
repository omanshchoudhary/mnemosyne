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

Work in progress. The bottom half of the stack is in:

| Layer | What it does |
|-------|----------------|
| **Disk manager** | Positional 4 KB page I/O, allocate, `fsync` |
| **Pages** | Little-endian typed reads/writes; slotted layout for variable records |
| **Meta page** | Page 0, holding magic, version and the B+tree root pointer |
| **Buffer pool** | Fixed frames, pin/unpin, LRU eviction, dirty flush |
| **B+tree** | Insert, lookup, delete, range scan, splits and merges |

Still ahead: WAL, MVCC / snapshot isolation, ARIES-style recovery, version GC.

Two limits in the tree are deliberate. A merged-away page is not reused yet, so
the file only grows; the free list belongs with GC. And an underfull node whose
sibling will not fit is left underfull rather than redistributed, which costs
some density and no correctness.

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
```

Pages are 4096 bytes. Slot entries are 4 bytes (`u16` offset + `u16` length). Leaves
store `RecordId` (page + slot) plus the key. Internal nodes store child page ids
and separators. Page 0 is reserved as the meta page, so `PageId(0)` is never a
tree node.

## Use

```rust
use mnemosyne::btree::BTree;
use mnemosyne::page::{PageId, RecordId};

let mut tree = BTree::open(path, /* frame_count */ 32)?;
tree.insert(b"alpha", RecordId { page: PageId(3), slot: 1 })?;

let rid = tree.lookup(b"alpha")?;
let range = tree.scan(b"a", b"z")?;
let gone = tree.delete(b"alpha")?;
```

`open` creates a fresh file (meta page + empty root leaf) or reopens an existing
one. `insert` overwrites the record id if the key already exists. `scan` is
half-open: `[start, end)`. `delete` returns whether the key was there.

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
