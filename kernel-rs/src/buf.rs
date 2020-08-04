use crate::sleeplock::Sleeplock;

pub struct Buf {
    /// has data been read from disk?
    valid: i32,

    /// does disk "own" buf?
    disk: i32,
    dev: u32,
    blockno: u32,
    pub lock: Sleeplock,
    refcnt: u32,

    /// LRU cache list
    pub prev: *mut Buf,
    pub next: *mut Buf,

    /// disk queue
    pub qnext: *mut Buf,
    pub data: [u8; 1024],
}

impl Buf {
    pub fn getvalid(&mut self) -> i32 {
        self.valid
    }
    pub fn setvalid(&mut self, valid: i32) {
        self.valid = valid;
    }
    pub fn getdisk(&mut self) -> i32 {
        self.disk
    }
    pub fn setdisk(&mut self, disk: i32) {
        self.disk = disk;
    }
    pub fn getdev(&mut self) -> u32 {
        self.dev
    }
    pub fn setdev(&mut self, dev: u32) {
        self.dev = dev;
    }
    pub fn getblockno(&mut self) -> u32 {
        self.blockno
    }
    pub fn setblockno(&mut self, blockno: u32) {
        self.blockno = blockno;
    }
    pub fn getrefcnt(&mut self) -> u32 {
        self.refcnt
    }
    pub fn setrefcnt(&mut self, refcnt: u32) {
        self.refcnt = refcnt;
    }
    pub fn decrefcnt(&mut self) {
        self.refcnt = self.refcnt.wrapping_sub(1);
    }
    pub fn increfcnt(&mut self) {
        self.refcnt = self.refcnt.wrapping_add(1);
    }
}
