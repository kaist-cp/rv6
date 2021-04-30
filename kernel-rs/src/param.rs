/// Maximum number of processes.
pub const NPROC: usize = 64;

/// Maximum number of CPUs.
pub const NCPU: usize = 8;

/// Open files per process.
pub const NOFILE: usize = 16;

/// Open files per system.
pub const NFILE: usize = 100;

/// Maximum number of active i-nodes.
pub const NINODE: usize = 50;

/// Maximum major device number.
pub const NDEV: usize = 10;

/// Device number of file system root disk.
pub const ROOTDEV: u32 = 1;

/// Max exec arguments.
pub const MAXARG: usize = 32;

/// Block Size.
pub const BSIZE: usize = 1024;

/// Max # of blocks any FS op writes.
/// Will be handled in #31.
pub const MAXOPBLOCKS: usize = 10;

/// Max data blocks in on-disk log.
pub const LOGSIZE: usize = MAXOPBLOCKS * 3;

/// Size of disk block cache.
pub const NBUF: usize = MAXOPBLOCKS * 3;

/// Maximum file path name.
pub const MAXPATH: usize = 128;

/// Maximum length of process name.
pub const MAXPROCNAME: usize = 16;
