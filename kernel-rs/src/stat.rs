use crate::libc;
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;

#[derive(Copy, Clone)]
pub struct Stat {
    /// File system's disk device
    pub dev: libc::c_int,
    /// Inode number
    pub ino: uint,
    /// Type of file
    pub type_0: libc::c_short,
    /// Number of links to file
    pub nlink: libc::c_short,
    /// Size of file in bytes
    pub size: uint64,
}
