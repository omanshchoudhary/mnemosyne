pub const PAGE_SIZE: usize = 4096;

// TODO: drop both allow(dead_code) once slotted.rs uses these.
#[allow(dead_code)]
pub struct Page {
    data: [u8; PAGE_SIZE],
}

#[allow(dead_code)]
impl Page {
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

    pub(crate) fn write_bytes(&mut self, offset: usize, src: &[u8]) {
        self.data[offset..offset + src.len()].copy_from_slice(src);
    }
}
