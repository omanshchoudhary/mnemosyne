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

Work in progress. Built so far: fixed 4 KB pages with typed reads and writes at
explicit offsets, a slotted layout storing variable length records addressed by
slot id, and a disk manager that reads, writes, allocates and syncs pages.

## Build

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

Warnings are errors. A pre-commit hook runs formatting, lints and tests before
anything lands.
