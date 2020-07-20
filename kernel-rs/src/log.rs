use crate::libc;
extern "C" {
    pub type cpu;
    #[no_mangle]
    fn bread(_: uint, _: uint) -> *mut buf;
    #[no_mangle]
    fn brelse(_: *mut buf);
    #[no_mangle]
    fn bwrite(_: *mut buf);
    #[no_mangle]
    fn bpin(_: *mut buf);
    #[no_mangle]
    fn bunpin(_: *mut buf);
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    #[no_mangle]
    fn sleep(_: *mut libc::c_void, _: *mut spinlock);
    #[no_mangle]
    fn wakeup(_: *mut libc::c_void);
    // spinlock.c
    #[no_mangle]
    fn acquire(_: *mut spinlock);
    #[no_mangle]
    fn initlock(_: *mut spinlock, _: *mut libc::c_char);
    #[no_mangle]
    fn release(_: *mut spinlock);
    #[no_mangle]
    fn memmove(_: *mut libc::c_void, _: *const libc::c_void, _: uint) -> *mut libc::c_void;
}
pub type uint = libc::c_uint;
pub type uchar = libc::c_uchar;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct buf {
    pub valid: libc::c_int,
    pub disk: libc::c_int,
    pub dev: uint,
    pub blockno: uint,
    pub lock: sleeplock,
    pub refcnt: uint,
    pub prev: *mut buf,
    pub next: *mut buf,
    pub qnext: *mut buf,
    pub data: [uchar; 1024],
}
// Long-term locks for processes
#[derive(Copy, Clone)]
#[repr(C)]
pub struct sleeplock {
    pub locked: uint,
    pub lk: spinlock,
    pub name: *mut libc::c_char,
    pub pid: libc::c_int,
}
// Mutual exclusion lock.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct spinlock {
    pub locked: uint,
    pub name: *mut libc::c_char,
    pub cpu: *mut cpu,
}
// block size
// Disk layout:
// [ boot block | super block | log | inode blocks |
//                                          free bit map | data blocks]
//
// mkfs computes the super block and builds an initial file system. The
// super block describes the disk layout:
#[derive(Copy, Clone)]
#[repr(C)]
pub struct superblock {
    pub magic: uint,
    pub size: uint,
    pub nblocks: uint,
    pub ninodes: uint,
    pub nlog: uint,
    pub logstart: uint,
    pub inodestart: uint,
    pub bmapstart: uint,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct log {
    pub lock: spinlock,
    pub start: libc::c_int,
    pub size: libc::c_int,
    pub outstanding: libc::c_int,
    pub committing: libc::c_int,
    pub dev: libc::c_int,
    pub lh: logheader,
}
// Simple logging that allows concurrent FS system calls.
//
// A log transaction contains the updates of multiple FS system
// calls. The logging system only commits when there are
// no FS system calls active. Thus there is never
// any reasoning required about whether a commit might
// write an uncommitted system call's updates to disk.
//
// A system call should call begin_op()/end_op() to mark
// its start and end. Usually begin_op() just increments
// the count of in-progress FS system calls and returns.
// But if it thinks the log is close to running out, it
// sleeps until the last outstanding end_op() commits.
//
// The log is a physical re-do log containing disk blocks.
// The on-disk log format:
//   header block, containing block #s for block A, B, C, ...
//   block A
//   block B
//   block C
//   ...
// Log appends are synchronous.
// Contents of the header block, used for both the on-disk header block
// and to keep track in memory of logged block# before commit.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct logheader {
    pub n: libc::c_int,
    pub block: [libc::c_int; 30],
}
// maximum number of processes
// maximum number of CPUs
// open files per process
// open files per system
// maximum number of active i-nodes
// maximum major device number
// device number of file system root disk
// max exec arguments
pub const MAXOPBLOCKS: libc::c_int = 10 as libc::c_int;
// max # of blocks any FS op writes
pub const LOGSIZE: libc::c_int = MAXOPBLOCKS * 3 as libc::c_int;
// On-disk file system format.
// Both the kernel and user programs use this header file.
// root i-number
pub const BSIZE: libc::c_int = 1024 as libc::c_int;
#[no_mangle]
pub static mut log: log = log {
    lock: spinlock {
        locked: 0,
        name: 0 as *const libc::c_char as *mut libc::c_char,
        cpu: 0 as *const cpu as *mut cpu,
    },
    start: 0,
    size: 0,
    outstanding: 0,
    committing: 0,
    dev: 0,
    lh: logheader {
        n: 0,
        block: [0; 30],
    },
};
// log.c
#[no_mangle]
pub unsafe extern "C" fn initlog(mut dev: libc::c_int, mut sb: *mut superblock) {
    if ::core::mem::size_of::<logheader>() as libc::c_ulong >= BSIZE as libc::c_ulong {
        panic(
            b"initlog: too big logheader\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    initlock(
        &mut log.lock,
        b"log\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    log.start = (*sb).logstart as libc::c_int;
    log.size = (*sb).nlog as libc::c_int;
    log.dev = dev;
    recover_from_log();
}
// Copy committed blocks from log to their home location
unsafe extern "C" fn install_trans() {
    let mut tail: libc::c_int = 0; // read log block
    tail = 0 as libc::c_int; // read dst
    while tail < log.lh.n {
        let mut lbuf: *mut buf = bread(
            log.dev as uint,
            (log.start + tail + 1 as libc::c_int) as uint,
        ); // copy block to dst
        let mut dbuf: *mut buf = bread(log.dev as uint, log.lh.block[tail as usize] as uint); // write dst to disk
        memmove(
            (*dbuf).data.as_mut_ptr() as *mut libc::c_void,
            (*lbuf).data.as_mut_ptr() as *const libc::c_void,
            BSIZE as uint,
        );
        bwrite(dbuf);
        bunpin(dbuf);
        brelse(lbuf);
        brelse(dbuf);
        tail += 1
    }
}
// Read the log header from disk into the in-memory log header
unsafe extern "C" fn read_head() {
    let mut buf: *mut buf = bread(log.dev as uint, log.start as uint);
    let mut lh: *mut logheader = (*buf).data.as_mut_ptr() as *mut logheader;
    let mut i: libc::c_int = 0;
    log.lh.n = (*lh).n;
    i = 0 as libc::c_int;
    while i < log.lh.n {
        log.lh.block[i as usize] = (*lh).block[i as usize];
        i += 1
    }
    brelse(buf);
}
// Write in-memory log header to disk.
// This is the true point at which the
// current transaction commits.
unsafe extern "C" fn write_head() {
    let mut buf: *mut buf = bread(log.dev as uint, log.start as uint); // if committed, copy from log to disk
    let mut hb: *mut logheader = (*buf).data.as_mut_ptr() as *mut logheader;
    let mut i: libc::c_int = 0;
    (*hb).n = log.lh.n;
    i = 0 as libc::c_int;
    while i < log.lh.n {
        (*hb).block[i as usize] = log.lh.block[i as usize];
        i += 1
    }
    bwrite(buf);
    brelse(buf);
}
unsafe extern "C" fn recover_from_log() {
    read_head();
    install_trans();
    log.lh.n = 0 as libc::c_int;
    write_head();
    // clear the log
}
// called at the start of each FS system call.
#[no_mangle]
pub unsafe extern "C" fn begin_op() {
    acquire(&mut log.lock);
    loop {
        if log.committing != 0 {
            sleep(&mut log as *mut log as *mut libc::c_void, &mut log.lock);
        } else if log.lh.n + (log.outstanding + 1 as libc::c_int) * MAXOPBLOCKS > LOGSIZE {
            // this op might exhaust log space; wait for commit.
            sleep(&mut log as *mut log as *mut libc::c_void, &mut log.lock);
        } else {
            log.outstanding += 1 as libc::c_int;
            release(&mut log.lock);
            break;
        }
    }
}
// called at the end of each FS system call.
// commits if this was the last outstanding operation.
#[no_mangle]
pub unsafe extern "C" fn end_op() {
    let mut do_commit: libc::c_int = 0 as libc::c_int;
    acquire(&mut log.lock);
    log.outstanding -= 1 as libc::c_int;
    if log.committing != 0 {
        panic(b"log.committing\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if log.outstanding == 0 as libc::c_int {
        do_commit = 1 as libc::c_int;
        log.committing = 1 as libc::c_int
    } else {
        // begin_op() may be waiting for log space,
        // and decrementing log.outstanding has decreased
        // the amount of reserved space.
        wakeup(&mut log as *mut log as *mut libc::c_void);
    }
    release(&mut log.lock);
    if do_commit != 0 {
        // call commit w/o holding locks, since not allowed
        // to sleep with locks.
        commit();
        acquire(&mut log.lock);
        log.committing = 0 as libc::c_int;
        wakeup(&mut log as *mut log as *mut libc::c_void);
        release(&mut log.lock);
    };
}
// Copy modified blocks from cache to log.
unsafe extern "C" fn write_log() {
    let mut tail: libc::c_int = 0; // log block
    tail = 0 as libc::c_int; // cache block
    while tail < log.lh.n {
        let mut to: *mut buf = bread(
            log.dev as uint,
            (log.start + tail + 1 as libc::c_int) as uint,
        ); // write the log
        let mut from: *mut buf = bread(log.dev as uint, log.lh.block[tail as usize] as uint); // Write modified blocks from cache to log
        memmove(
            (*to).data.as_mut_ptr() as *mut libc::c_void,
            (*from).data.as_mut_ptr() as *const libc::c_void,
            BSIZE as uint,
        );
        bwrite(to);
        brelse(from);
        brelse(to);
        tail += 1
    }
}
unsafe extern "C" fn commit() {
    if log.lh.n > 0 as libc::c_int {
        write_log();
        // Erase the transaction from the log
        write_head(); // Write header to disk -- the real commit
        install_trans(); // Now install writes to home locations
        log.lh.n = 0 as libc::c_int;
        write_head();
    };
}
// Caller has modified b->data and is done with the buffer.
// Record the block number and pin in the cache by increasing refcnt.
// commit()/write_log() will do the disk write.
//
// log_write() replaces bwrite(); a typical use is:
//   bp = bread(...)
//   modify bp->data[]
//   log_write(bp)
//   brelse(bp)
#[no_mangle]
pub unsafe extern "C" fn log_write(mut b: *mut buf) {
    let mut i: libc::c_int = 0;
    if log.lh.n >= LOGSIZE || log.lh.n >= log.size - 1 as libc::c_int {
        panic(
            b"too big a transaction\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        );
    }
    if log.outstanding < 1 as libc::c_int {
        panic(
            b"log_write outside of trans\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    acquire(&mut log.lock);
    i = 0 as libc::c_int;
    while i < log.lh.n {
        if log.lh.block[i as usize] as libc::c_uint == (*b).blockno {
            break;
        }
        i += 1
    }
    log.lh.block[i as usize] = (*b).blockno as libc::c_int;
    if i == log.lh.n {
        // Add new block to log?
        bpin(b);
        log.lh.n += 1
    }
    release(&mut log.lock);
}
