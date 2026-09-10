#![allow(dead_code)]

use crate::page::{PageId, RecordId};

const OFF_BEGIN: usize = 0;
const OFF_END: usize = 8;
const OFF_PREV: usize = 16;
const VERSION_HEADER_SIZE: usize = OFF_PREV + RecordId::SIZE;

pub(crate) const NO_END: u64 = u64::MAX;

const NO_PREV: RecordId = RecordId {
    page: PageId(0),
    slot: 0,
};

pub(crate) fn encode_version(begin: u64, prev: Option<RecordId>, value: &[u8]) -> Vec<u8> {
    let mut entry: Vec<u8> = Vec::with_capacity(VERSION_HEADER_SIZE + value.len());
    entry.extend_from_slice(&begin.to_le_bytes());
    entry.extend_from_slice(&NO_END.to_le_bytes());
    entry.extend_from_slice(&prev.unwrap_or(NO_PREV).to_bytes());
    entry.extend_from_slice(value);

    entry
}

pub(crate) fn version_begin(version: &[u8]) -> u64 {
    u64::from_le_bytes(version[..OFF_END].try_into().unwrap())
}

pub(crate) fn version_end(version: &[u8]) -> u64 {
    u64::from_le_bytes(version[OFF_END..OFF_PREV].try_into().unwrap())
}

pub(crate) fn version_prev(version: &[u8]) -> Option<RecordId> {
    match RecordId::from_bytes(&version[OFF_PREV..VERSION_HEADER_SIZE]) {
        NO_PREV => None,
        prev => Some(prev),
    }
}

pub(crate) fn version_value(version: &[u8]) -> &[u8] {
    &version[VERSION_HEADER_SIZE..]
}

pub(crate) fn set_version_end(version: &mut [u8], end: u64) {
    version[OFF_END..OFF_PREV].copy_from_slice(&end.to_le_bytes());
}
