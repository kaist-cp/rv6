use crate::libc;
/// maximum number of processes
pub const NPROC: i32 = 64;
/// open files per process
pub const NOFILE: i32 = 16;
/// open files per system
pub const NFILE: i32 = 100;
/// maximum number of active i-nodes
pub const NINODE: i32 = 50;
/// maximum major device number
pub const NDEV: i32 = 10;
/// device number of file system root disk
pub const ROOTDEV: i32 = 1;
/// max exec arguments
pub const MAXARG: i32 = 32;
/// max # of blocks any FS op writes
/// Will be handled in #31
pub const MAXOPBLOCKS: libc::c_int = 10 as libc::c_int;
/// max data blocks in on-disk log
pub const LOGSIZE: i32 = MAXOPBLOCKS * 3;
/// size of disk block cache
pub const NBUF: libc::c_int = MAXOPBLOCKS * 3 as libc::c_int;
/// maximum file path name
pub const MAXPATH: i32 = 128;
