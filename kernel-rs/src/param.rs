/// maximum number of processes
pub const NPROC: usize = 64;

/// maximum number of CPUs
pub const NCPU: usize = 8;

/// open files per process
pub const NOFILE: usize = 16;

/// open files per system
pub const NFILE: usize = 100;

/// maximum number of active i-nodes
pub const NINODE: usize = 50;

/// maximum major device number
pub const NDEV: usize = 10;

/// device number of file system root disk
pub const ROOTDEV: i32 = 1;

/// max exec arguments
pub const MAXARG: usize = 32;

/// max # of blocks any FS op writes
/// Will be handled in #31
pub const MAXOPBLOCKS: usize = 10;

/// max data blocks in on-disk log
pub const LOGSIZE: usize = MAXOPBLOCKS * 3;

/// size of disk block cache
pub const NBUF: usize = MAXOPBLOCKS * 3;

/// size of file system in blocks
pub const FSSIZE: i32 = 1000;

/// maximum file path name
pub const MAXPATH: usize = 128;
