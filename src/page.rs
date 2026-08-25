pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PageId(pub u64);

impl PageId {
    // page n lives at byte n × 4096 in the file
    pub fn offset(self) -> u64 {
        self.0 * PAGE_SIZE as u64
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_page_is_all_zeros() {
        let page = Page::new();
        assert_eq!(page.data.len(), PAGE_SIZE);
        assert!(page.data.iter().all(|&b| b == 0));
    }

    #[test]
    fn u16_round_trip() {
        let mut page = Page::new();
        page.write_u16(0, 1);
        page.write_u16(100, u16::MAX);
        assert_eq!(page.read_u16(0), 1);
        assert_eq!(page.read_u16(100), u16::MAX);
    }

    #[test]
    fn u32_round_trip() {
        let mut page = Page::new();
        page.write_u32(8, 123_456_789);
        assert_eq!(page.read_u32(8), 123_456_789);
    }

    #[test]
    fn u64_round_trip() {
        let mut page = Page::new();
        page.write_u64(16, u64::MAX);
        page.write_u64(32, 0);
        assert_eq!(page.read_u64(16), u64::MAX);
        assert_eq!(page.read_u64(32), 0);
    }

    #[test]
    fn integers_are_stored_little_endian() {
        // the low byte lands first; this is what catches a be/le mismatch
        let mut page = Page::new();
        page.write_u16(0, 0x0102);
        assert_eq!(&page.data[0..2], &[0x02, 0x01]);

        page.write_u32(4, 0x0102_0304);
        assert_eq!(&page.data[4..8], &[0x04, 0x03, 0x02, 0x01]);

        page.write_u64(8, 0x0102_0304_0506_0708);
        assert_eq!(
            &page.data[8..16],
            &[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]
        );
    }

    #[test]
    fn writes_do_not_touch_neighbouring_bytes() {
        let mut page = Page::new();
        page.write_u16(10, u16::MAX);
        assert_eq!(page.data[9], 0);
        assert_eq!(page.data[12], 0);
    }

    #[test]
    fn bytes_round_trip() {
        let mut page = Page::new();
        let record = b"hello mnemosyne";
        page.write_bytes(64, record);
        assert_eq!(page.read_bytes(64, record.len()), record);
    }

    #[test]
    fn overwriting_replaces_the_old_value() {
        let mut page = Page::new();
        page.write_u32(0, 42);
        page.write_u32(0, 7);
        assert_eq!(page.read_u32(0), 7);
    }

    #[test]
    fn page_id_maps_to_a_file_offset() {
        assert_eq!(PageId(0).offset(), 0);
        assert_eq!(PageId(1).offset(), PAGE_SIZE as u64);
        assert_eq!(PageId(5).offset(), 20_480);
    }

    #[test]
    #[should_panic]
    fn reading_past_the_end_panics() {
        let page = Page::new();
        page.read_u32(PAGE_SIZE - 2);
    }
}
