use cfg_if::cfg_if;

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

cfg_if! {
    if #[cfg(feature = "lfs")] {
        // TODO: The following may be actually unknown at compile time.

        /// Size of a segment in blocks
        ///
        /// An optimal size of segments for LFS is dependent to
        /// the performance of a disk and a desired effective bandwith of developers.
        /// Check the formula for getting the size of segments here:
        /// https://pages.cs.wisc.edu/~remzi/OSTEP/file-lfs.pdf
        ///
        /// TODO: optimize the size of the segment.
        /// Note that this is much smaller than in sprite-lfs. sprite-lfs uses segments
        /// of size 512KB ~ 1MB.
        pub const SEGSIZE: usize = 10;

        /// Size of the imap in blocks
        pub const IMAPSIZE: usize = 1;

        /// Size of the segment usage table in bytes
        pub const SEGTABLESIZE: usize = 64;
    } else {
        /// Max data blocks in on-disk log.
        pub const LOGSIZE: usize = MAXOPBLOCKS * 3;
    }
}

/// Size of disk block cache.
pub const NBUF: usize = MAXOPBLOCKS * 3;

/// Maximum file path name.
pub const MAXPATH: usize = 128;

/// Maximum length of process name.
pub const MAXPROCNAME: usize = 16;
