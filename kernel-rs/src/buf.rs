use crate::sleeplock::Sleeplock;
use core::sync::atomic::AtomicI32;

pub struct Buf {
    /// has data been read from disk?
    pub valid: i32,

    /// does disk "own" buf?
    pub disk: AtomicI32,
    pub dev: u32,
    pub blockno: u32,
    pub lock: Sleeplock,
    pub refcnt: u32,

    /// LRU cache list
    pub prev: *mut Buf,
    pub next: *mut Buf,

    /// disk queue
    qnext: *mut Buf,
    pub data: [u8; 1024],
}
