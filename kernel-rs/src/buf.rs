use crate::sleeplock::Sleeplock;
use crate::fs::BSIZE;

pub struct Buf {
    /// Has data been read from disk?
    pub valid: i32,

    /// Does disk "own" buf?
    pub disk: i32,
    pub dev: u32,
    pub blockno: u32,
    pub lock: Sleeplock,
    pub refcnt: u32,

    /// LRU cache list.
    pub prev: *mut Buf,
    pub next: *mut Buf,

    /// Disk queue.
    qnext: *mut Buf,
    pub data: [u8; BSIZE],
}
