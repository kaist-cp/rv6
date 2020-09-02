use crate::sleeplock::SleeplockWIP;

pub struct Buf {
    /// has data been read from disk?
    pub valid: i32,

    /// does disk "own" buf?
    pub disk: i32,
    pub dev: u32,
    pub blockno: u32,
    pub refcnt: u32,

    /// LRU cache list
    pub prev: *mut Buf,
    pub next: *mut Buf,
    
    /// disk queue
    qnext: *mut BufBlock,
    pub data: SleeplockWIP<BufBlock>,
}

pub struct BufBlock {
    pub inner: [u8; 1024],
}
