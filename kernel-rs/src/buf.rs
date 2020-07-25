use crate::{libc, sleeplock};
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
pub type uchar = libc::c_uchar;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct Buf {
    /// has data been read from disk?
    pub valid: libc::c_int,
    /// does disk "own" buf?
    pub disk: libc::c_int,
    pub dev: uint,
    pub blockno: uint,
    pub lock: sleeplock::Sleeplock,
    pub refcnt: uint,
    /// LRU cache list
    pub prev: *mut Buf,
    pub next: *mut Buf,
    /// disk queue
    pub qnext: *mut Buf,
    pub data: [uchar; 1024],
}
