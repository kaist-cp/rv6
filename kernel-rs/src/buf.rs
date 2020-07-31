use crate::sleeplock::Sleeplock;
#[derive(Copy, Clone)]
pub struct Buf {
    /// has data been read from disk?
    pub valid: i32,
    /// does disk "own" buf?
    pub disk: i32,
    pub dev: u32,
    pub blockno: u32,
    pub lock: Sleeplock,
    pub refcnt: u32,
    /// LRU cache list
    pub prev: *mut Buf,
    pub next: *mut Buf,
    /// disk queue
    pub qnext: *mut Buf,
    pub data: [u8; 1024],
}
