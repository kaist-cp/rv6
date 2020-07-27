#[derive(Copy, Clone)]
pub struct Stat {
    /// File system's disk device
    pub dev: i32,
    /// Inode number
    pub ino: u32,
    /// Type of file
    pub type_0: i16,
    /// Number of links to file
    pub nlink: i16,
    /// Size of file in bytes
    pub size: u64,
}
