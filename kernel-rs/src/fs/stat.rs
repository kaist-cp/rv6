use zerocopy::AsBytes;

#[derive(Copy, Clone, AsBytes)]
#[repr(C)]
pub struct Stat {
    /// File system's disk device
    pub dev: i32,

    /// Inode number
    pub ino: u32,

    /// Type of file
    pub typ: u16,

    /// Number of links to file
    pub nlink: i16,

    /// Padding for safetly serializing the struct
    pub _padding: u32,

    /// Size of file in bytes
    pub size: usize,
}
