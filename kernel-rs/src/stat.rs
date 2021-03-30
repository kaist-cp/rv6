use zerocopy::{AsBytes, FromBytes};

#[derive(Copy, Clone, AsBytes, FromBytes)]
// repr(packed) is required for AsBytes.
// https://docs.rs/zerocopy/0.3.0/zerocopy/trait.AsBytes.html
#[repr(packed)]
pub struct Stat {
    /// File system's disk device
    pub dev: i32,

    /// Inode number
    pub ino: u32,

    /// Type of file
    pub typ: u16,

    /// Number of links to file
    pub nlink: i16,

    /// Size of file in bytes
    pub size: usize,
}
