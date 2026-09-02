// TODO: drop once the buffer pool and B+tree call into this layer.
#![allow(dead_code)]

use crate::error::{Error, Result};
use crate::page::{PAGE_SIZE, Page};

pub(crate) type SlotId = u16;

const OFF_LSN: usize = 0; // u64 - log sequence number (WAL)
const OFF_CHECKSUM: usize = 8; // u32 - CRC32 of the rest of the page
const OFF_PAGE_TYPE: usize = 12; // u8  - slotted / leaf / internal
const OFF_FLAGS: usize = 13; // u8  - reserved
const OFF_SLOT_COUNT: usize = 14; // u16 - number of slot entries
const OFF_FREE_PTR: usize = 16; // u16 - start of the data region
const OFF_FREE_BYTES: usize = 18; // u16 - free bytes, excluding fragmentation
const OFF_FRAG_BYTES: usize = 20; // u16 - bytes stranded by removals, freed by compaction
const OFF_RESERVED: usize = 22; // 2 bytes spare
const OFF_LINK: usize = 24; // 8 bytes for link(PageID)
pub(crate) const HEADER_SIZE: usize = 32;

pub(crate) const SLOT_SIZE: usize = 4; // offset (2 bytes) + length (2 bytes)

const PAGE_TYPE_SLOTTED: u8 = 1;

impl Page {
    // A zeroed page is not a valid slotted page. Call this once on allocation.
    pub(crate) fn init_slotted(&mut self) {
        self.write_u64(OFF_LSN, 0);
        self.write_u32(OFF_CHECKSUM, 0);
        self.write_u8(OFF_PAGE_TYPE, PAGE_TYPE_SLOTTED);
        self.write_u8(OFF_FLAGS, 0);
        self.write_u16(OFF_SLOT_COUNT, 0);
        // data grows down from the end, so the free pointer starts past the last byte
        self.write_u16(OFF_FREE_PTR, PAGE_SIZE as u16);
        self.write_u16(OFF_FREE_BYTES, (PAGE_SIZE - HEADER_SIZE) as u16);
        self.write_u16(OFF_FRAG_BYTES, 0);
        self.write_u16(OFF_RESERVED, 0);
        self.write_u64(OFF_LINK, 0);
    }

    pub(crate) fn slot_count(&self) -> u16 {
        self.read_u16(OFF_SLOT_COUNT)
    }

    pub(crate) fn free_space(&self) -> usize {
        self.read_u16(OFF_FREE_BYTES) as usize
    }

    // bytes a compaction would hand back, on top of free_space
    pub(crate) fn frag_space(&self) -> usize {
        self.read_u16(OFF_FRAG_BYTES) as usize
    }

    pub(crate) fn page_type(&self) -> u8 {
        self.read_u8(OFF_PAGE_TYPE)
    }

    pub(crate) fn set_page_type(&mut self, page_type: u8) {
        self.write_u8(OFF_PAGE_TYPE, page_type);
    }

    pub(crate) fn link(&self) -> u64 {
        self.read_u64(OFF_LINK)
    }

    pub(crate) fn set_link(&mut self, value: u64) {
        self.write_u64(OFF_LINK, value);
    }

    pub(crate) fn append_slot(&mut self, record: &[u8]) -> Result<SlotId> {
        if self.free_space() < SLOT_SIZE + record.len() {
            return Err(Error::PageFull {
                size: record.len() + SLOT_SIZE,
                free: self.free_space(),
            });
        }

        // ids are 0-based, so with N slots the next free id is N
        let new_slot = self.slot_count();
        let record_offset = self.read_u16(OFF_FREE_PTR) as usize - record.len();
        let free_left = self.free_space() - record.len() - SLOT_SIZE;

        self.write_bytes(record_offset, record);
        self.write_slot(new_slot, record_offset as u16, record.len() as u16);

        // header last, so an early return can never leave a half-updated page
        self.write_u16(OFF_FREE_PTR, record_offset as u16);
        self.write_u16(OFF_FREE_BYTES, free_left as u16);
        self.write_u16(OFF_SLOT_COUNT, new_slot + 1);

        Ok(new_slot)
    }

    pub(crate) fn insert_slot_at(&mut self, slot: SlotId, record: &[u8]) -> Result<()> {
        if slot > self.slot_count() {
            return Err(Error::NoSuchSlot(slot));
        }

        if self.free_space() < SLOT_SIZE + record.len() {
            return Err(Error::PageFull {
                size: record.len() + SLOT_SIZE,
                free: self.free_space(),
            });
        }
        for i in (slot..self.slot_count()).rev() {
            let (offset, len) = self.read_slot(i);
            self.write_slot(i + 1, offset, len);
        }

        let record_offset = self.read_u16(OFF_FREE_PTR) as usize - record.len();
        let free_left = self.free_space() - record.len() - SLOT_SIZE;

        self.write_bytes(record_offset, record);
        self.write_slot(slot, record_offset as u16, record.len() as u16);

        // header updation
        self.write_u16(OFF_FREE_PTR, record_offset as u16);
        self.write_u16(OFF_FREE_BYTES, free_left as u16);
        self.write_u16(OFF_SLOT_COUNT, self.slot_count() + 1);

        Ok(())
    }

    pub(crate) fn remove_slot_at(&mut self, slot: SlotId) -> Result<()> {
        // can't remove a thing which does not exist
        if slot >= self.slot_count() {
            return Err(Error::NoSuchSlot(slot));
        }
        // read the size of record
        let (_, len) = self.read_slot(slot);

        // shift records by 1 place left
        for i in slot + 1..self.slot_count() {
            let (offset, len) = self.read_slot(i);
            self.write_slot(i - 1, offset, len);
        }

        self.write_u16(OFF_FREE_BYTES, (self.free_space() + SLOT_SIZE) as u16);
        self.write_u16(OFF_FRAG_BYTES, (self.frag_space() + len as usize) as u16);
        self.write_u16(OFF_SLOT_COUNT, self.slot_count() - 1);

        Ok(())
    }

    pub(crate) fn slot_bytes(&self, slot: SlotId) -> Result<&[u8]> {
        if slot >= self.slot_count() {
            return Err(Error::NoSuchSlot(slot));
        }

        let (offset, len) = self.read_slot(slot);
        // offset 0 lands inside the header, so it can never be a live slot
        if offset == 0 {
            return Err(Error::SlotDeleted(slot));
        }

        Ok(self.read_bytes(offset as usize, len as usize))
    }

    pub(crate) fn slot_bytes_mut(&mut self, slot: SlotId) -> Result<&mut [u8]> {
        if slot >= self.slot_count() {
            return Err(Error::NoSuchSlot(slot));
        }

        let (offset, len) = self.read_slot(slot);
        if offset == 0 {
            return Err(Error::SlotDeleted(slot));
        }

        Ok(self.bytes_mut(offset as usize, len as usize))
    }

    pub(crate) fn tombstone_slot(&mut self, slot: SlotId) -> Result<()> {
        if slot >= self.slot_count() {
            return Err(Error::NoSuchSlot(slot));
        }

        let (offset, _) = self.read_slot(slot);
        if offset == 0 {
            return Err(Error::SlotDeleted(slot));
        }

        // tombstone only: the slot bytes stay put until compaction reclaims them
        self.write_slot(slot, 0, 0);
        Ok(())
    }

    fn slot_entry_pos(slot: SlotId) -> usize {
        HEADER_SIZE + slot as usize * SLOT_SIZE
    }

    fn read_slot(&self, slot: SlotId) -> (u16, u16) {
        let pos = Self::slot_entry_pos(slot);
        (self.read_u16(pos), self.read_u16(pos + 2))
    }

    fn write_slot(&mut self, slot: SlotId, offset: u16, len: u16) {
        let pos = Self::slot_entry_pos(slot);
        self.write_u16(pos, offset);
        self.write_u16(pos + 2, len);
    }
}

#[cfg(test)]
#[path = "slotted_tests.rs"]
mod tests;
