pub mod meta;
pub mod slotted;

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PageId(pub u64);

impl PageId {
    // page n lives at byte n × 4096 in the file
    pub fn offset(self) -> u64 {
        self.0 * PAGE_SIZE as u64
    }
}

// leaf in B+Tree stores this as value
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordId {
    pub page: PageId,
    pub slot: slotted::SlotId,
}

impl RecordId {
    pub(crate) const SIZE: usize = 10; // u64 page + u16 slot

    pub(crate) fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut raw = [0u8; Self::SIZE];
        raw[..8].copy_from_slice(&self.page.0.to_le_bytes());
        raw[8..].copy_from_slice(&self.slot.to_le_bytes());
        raw
    }

    pub(crate) fn from_bytes(raw: &[u8]) -> Self {
        Self {
            page: PageId(u64::from_le_bytes(raw[..8].try_into().unwrap())),
            slot: u16::from_le_bytes(raw[8..Self::SIZE].try_into().unwrap()),
        }
    }
}

// TODO: drop both allow(dead_code) once slotted.rs uses these.
#[allow(dead_code)]
pub struct Page {
    data: [u8; PAGE_SIZE],
}

impl Default for Page {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl Page {
    pub fn new() -> Self {
        Self {
            data: [0u8; PAGE_SIZE],
        }
    }

    pub(crate) fn from_bytes(data: [u8; PAGE_SIZE]) -> Self {
        Self { data }
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    pub(crate) fn read_u8(&self, offset: usize) -> u8 {
        self.data[offset]
    }

    pub(crate) fn read_u16(&self, offset: usize) -> u16 {
        // 2 bytes integer
        let raw: [u8; 2] = self.data[offset..offset + 2].try_into().unwrap();
        u16::from_le_bytes(raw)
    }

    pub(crate) fn read_u32(&self, offset: usize) -> u32 {
        // 4 bytes integer
        let raw: [u8; 4] = self.data[offset..offset + 4].try_into().unwrap();
        u32::from_le_bytes(raw)
    }

    pub(crate) fn read_u64(&self, offset: usize) -> u64 {
        // 8 bytes integer
        let raw: [u8; 8] = self.data[offset..offset + 8].try_into().unwrap();
        u64::from_le_bytes(raw)
    }

    pub(crate) fn write_u8(&mut self, offset: usize, value: u8) {
        self.data[offset] = value;
    }

    pub(crate) fn write_u16(&mut self, offset: usize, value: u16) {
        self.data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn write_u32(&mut self, offset: usize, value: u32) {
        self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn write_u64(&mut self, offset: usize, value: u64) {
        self.data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn read_bytes(&self, offset: usize, len: usize) -> &[u8] {
        &self.data[offset..offset + len]
    }

    pub(crate) fn bytes_mut(&mut self, offset: usize, len: usize) -> &mut [u8] {
        &mut self.data[offset..offset + len]
    }

    pub(crate) fn write_bytes(&mut self, offset: usize, src: &[u8]) {
        self.data[offset..offset + src.len()].copy_from_slice(src);
    }
}

#[cfg(test)]
#[path = "page_tests.rs"]
mod tests;
