#![allow(dead_code)]

use crate::buffer::BufferPool;
use crate::error::{Error, Result};
use crate::page::RecordId;
use crate::page::meta::META_PAGE_ID;

// append a new record in the heap page(tail)
pub(crate) fn insert(pool: &mut BufferPool, record: &[u8]) -> Result<RecordId> {
    let frame = pool.fetch_and_pin(META_PAGE_ID)?; // acess Meta Page from Db
    let tail = pool.page(frame).heap_tail();
    pool.unpin(frame)?;

    // tail exists so append the record
    if let Some(page_id) = tail {
        let frame = pool.fetch_and_pin(page_id)?;
        let appended = pool.page_for_write(frame).append_slot(record);
        pool.unpin(frame)?;

        match appended {
            Ok(slot) => {
                return Ok(RecordId {
                    page: page_id,
                    slot,
                });
            }
            Err(Error::PageFull { .. }) => {} // handled later by creating a new page
            Err(e) => return Err(e),
        }
    }

    // create a new page and make it tail and append the record
    let (page_id, frame) = pool.new_page()?;
    pool.page_for_write(frame).init_slotted();
    let appended = pool.page_for_write(frame).append_slot(record);
    pool.unpin(frame)?;

    let meta = pool.fetch_and_pin(META_PAGE_ID)?;
    pool.page_for_write(meta).set_heap_tail(page_id);
    pool.unpin(meta)?;

    Ok(RecordId {
        page: page_id,
        slot: appended?,
    })
}

pub(crate) fn get(pool: &mut BufferPool, rid: RecordId) -> Result<Vec<u8>> {
    let frame = pool.fetch_and_pin(rid.page)?;
    let out = pool.page(frame).slot_bytes(rid.slot).map(|r| r.to_vec());
    pool.unpin(frame)?;
    out
}

#[cfg(test)]
#[path = "heap_tests.rs"]
mod tests;
