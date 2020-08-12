/// maximum number of processes
pub const NPROC: usize = 64;

/// maximum number of CPUs
pub const NCPU: usize = 8;

/// open files per process
pub const NOFILE: usize = 16;

/// open files per system
pub const NFILE: i32 = 100;

/// maximum number of active i-nodes
pub const NINODE: i32 = 50;

/// maximum major device number
pub const NDEV: i32 = 10;

/// device number of file system root disk
pub const ROOTDEV: i32 = 1;

/// max exec arguments
pub const MAXARG: usize = 32;

/// max # of blocks any FS op writes
/// Will be handled in #31
pub const MAXOPBLOCKS: i32 = 10;

/// max data blocks in on-disk log
pub const LOGSIZE: i32 = MAXOPBLOCKS * 3;

/// size of disk block cache
pub const NBUF: i32 = MAXOPBLOCKS * 3;

/// size of file system in blocks
pub const FSSIZE: i32 = 1000;

/// maximum file path name
pub const MAXPATH: usize = 128;
