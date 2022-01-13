use arrayvec::ArrayVec;

use crate::{bio::BufUnlocked, param::LOGSIZE};

pub struct Log {
    dev: u32,
    start: i32,
    size: i32,

    /// In commit(), please wait.
    committing: bool,

    /// Contents of the header block, used to keep track in memory of logged block# before commit.
    bufs: ArrayVec<BufUnlocked, LOGSIZE>,
}
