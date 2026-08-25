use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("record of {size} bytes does not fit in {free} bytes of free space")]
    PageFull { size: usize, free: usize },

    #[error("slot {0} does not exist")]
    NoSuchSlot(u16),

    #[error("slot {0} was already deleted")]
    SlotDeleted(u16),

    #[error("page {requested} does not exist, the file holds {page_count} pages")]
    PageOutOfRange { requested: u64, page_count: u64 },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
