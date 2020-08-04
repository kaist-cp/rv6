/// Directory
pub const T_DIR: i32 = 1;

/// File
pub const T_FILE: i32 = 2;

/// Device
pub const T_DEVICE: i32 = 3;

#[derive(Default, Copy, Clone)]
pub struct Stat {
    /// File system's disk device
    pub dev: i32,

    /// Inode number
    pub ino: u32,

    /// Type of file
    pub typ: i16,

    /// Number of links to file
    pub nlink: i16,

    /// Size of file in bytes
    pub size: usize,
}
