use crate::libc; 
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;

#[derive(Copy, Clone)]
pub struct Stat {
    pub dev: libc::c_int,
    pub ino: uint,
    pub type_0: libc::c_short,
    pub nlink: libc::c_short,
    pub size: uint64,
}