use crate::{ libc, sleeplock }; 
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
pub type uchar = libc::c_uchar;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct Buf {
    pub valid: libc::c_int,
    pub disk: libc::c_int,
    pub dev: uint,
    pub blockno: uint,
    pub lock: sleeplock::Sleeplock,
    pub refcnt: uint,
    pub prev: *mut Buf,
    pub next: *mut Buf,
    pub qnext: *mut Buf,
    pub data: [uchar; 1024],
}