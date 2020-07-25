use crate::libc;
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;

#[derive(Copy, Clone)]
pub struct Stat {
    pub dev: libc::c_int,      // File system's disk device
    pub ino: uint,             // Inode number
    pub type_0: libc::c_short, // Type of file
    pub nlink: libc::c_short,  // Number of links to file
    pub size: uint64,          // Size of file in bytes
}
