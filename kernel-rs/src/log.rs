use crate::bio::{bpin, bread, brelse, bunpin, bwrite};
use crate::buf::Buf;
use crate::fs::{superblock, BSIZE};
use crate::libc;
use crate::param::{LOGSIZE, MAXOPBLOCKS};
use crate::proc::{cpu, sleep, wakeup};
use crate::spinlock::{acquire, initlock, release, Spinlock};
extern "C" {
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    #[no_mangle]
    fn memmove(_: *mut libc::c_void, _: *const libc::c_void, _: u32) -> *mut libc::c_void;
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct log {
    pub lock: Spinlock,
    pub start: i32,
    pub size: i32,
    /// how many FS sys calls are executing.
    pub outstanding: i32,
    /// in commit(), please wait.
    pub committing: i32,
    pub dev: i32,
    pub lh: logheader,
}
/// Simple logging that allows concurrent FS system calls.
///
/// A log transaction contains the updates of multiple FS system
/// calls. The logging system only commits when there are
/// no FS system calls active. Thus there is never
/// any reasoning required about whether a commit might
/// write an uncommitted system call's updates to disk.
///
/// A system call should call begin_op()/end_op() to mark
/// its start and end. Usually begin_op() just increments
/// the count of in-progress FS system calls and returns.
/// But if it thinks the log is close to running out, it
/// sleeps until the last outstanding end_op() commits.
///
/// The log is a physical re-do log containing disk blocks.
/// The on-disk log format:
///   header block, containing block #s for block A, B, C, ...
///   block A
///   block B
///   block C
///   ...
/// Log appends are synchronous.
/// Contents of the header block, used for both the on-disk header block
/// and to keep track in memory of logged block# before commit.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct logheader {
    pub n: i32,
    pub block: [i32; 30],
}
#[no_mangle]
pub static mut log: log = log {
    lock: Spinlock {
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
pub unsafe extern "C" fn initlog(mut dev: i32, mut sb: *mut superblock) {
    if ::core::mem::size_of::<logheader>() as u64 >= BSIZE as u64 {
        panic(
            b"initlog: too big logheader\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    initlock(
        &mut log.lock,
        b"log\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    log.start = (*sb).logstart as i32;
    log.size = (*sb).nlog as i32;
    log.dev = dev;
    recover_from_log();
}
/// Copy committed blocks from log to their home location
unsafe extern "C" fn install_trans() {
    let mut tail: i32 = 0; // read log block
    tail = 0; // read dst
    while tail < log.lh.n {
        let mut lbuf: *mut Buf = bread(log.dev as u32, (log.start + tail + 1 as i32) as u32); // copy block to dst
        let mut dbuf: *mut Buf = bread(log.dev as u32, log.lh.block[tail as usize] as u32); // write dst to disk
        memmove(
            (*dbuf).data.as_mut_ptr() as *mut libc::c_void,
            (*lbuf).data.as_mut_ptr() as *const libc::c_void,
            BSIZE as u32,
        );
        bwrite(dbuf);
        bunpin(dbuf);
        brelse(lbuf);
        brelse(dbuf);
        tail += 1
    }
}
/// Read the log header from disk into the in-memory log header
unsafe extern "C" fn read_head() {
    let mut buf: *mut Buf = bread(log.dev as u32, log.start as u32);
    let mut lh: *mut logheader = (*buf).data.as_mut_ptr() as *mut logheader;
    let mut i: i32 = 0;
    log.lh.n = (*lh).n;
    while i < log.lh.n {
        log.lh.block[i as usize] = (*lh).block[i as usize];
        i += 1
    }
    brelse(buf);
}
/// Write in-memory log header to disk.
/// This is the true point at which the
/// current transaction commits.
unsafe extern "C" fn write_head() {
    let mut buf: *mut Buf = bread(log.dev as u32, log.start as u32); // if committed, copy from log to disk
    let mut hb: *mut logheader = (*buf).data.as_mut_ptr() as *mut logheader;
    let mut i: i32 = 0;
    (*hb).n = log.lh.n;
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
    log.lh.n = 0 as i32;
    write_head();
    // clear the log
}
/// called at the start of each FS system call.
#[no_mangle]
pub unsafe extern "C" fn begin_op() {
    acquire(&mut log.lock);
    loop {
        if log.committing != 0 {
            sleep(&mut log as *mut log as *mut libc::c_void, &mut log.lock);
        } else if log.lh.n + (log.outstanding + 1 as i32) * MAXOPBLOCKS > LOGSIZE {
            // this op might exhaust log space; wait for commit.
            sleep(&mut log as *mut log as *mut libc::c_void, &mut log.lock);
        } else {
            log.outstanding += 1 as i32;
            release(&mut log.lock);
            break;
        }
    }
}
/// called at the end of each FS system call.
/// commits if this was the last outstanding operation.
#[no_mangle]
pub unsafe extern "C" fn end_op() {
    let mut do_commit: i32 = 0;
    acquire(&mut log.lock);
    log.outstanding -= 1 as i32;
    if log.committing != 0 {
        panic(b"log.committing\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if log.outstanding == 0 as i32 {
        do_commit = 1 as i32;
        log.committing = 1 as i32
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
        log.committing = 0 as i32;
        wakeup(&mut log as *mut log as *mut libc::c_void);
        release(&mut log.lock);
    };
}
/// Copy modified blocks from cache to log.
unsafe extern "C" fn write_log() {
    let mut tail: i32 = 0; // log block
    tail = 0; // cache block
    while tail < log.lh.n {
        let mut to: *mut Buf = bread(log.dev as u32, (log.start + tail + 1 as i32) as u32); // write the log
        let mut from: *mut Buf = bread(log.dev as u32, log.lh.block[tail as usize] as u32); // Write modified blocks from cache to log
        memmove(
            (*to).data.as_mut_ptr() as *mut libc::c_void,
            (*from).data.as_mut_ptr() as *const libc::c_void,
            BSIZE as u32,
        );
        bwrite(to);
        brelse(from);
        brelse(to);
        tail += 1
    }
}
unsafe extern "C" fn commit() {
    if log.lh.n > 0 as i32 {
        write_log();
        // Erase the transaction from the log
        write_head(); // Write header to disk -- the real commit
        install_trans(); // Now install writes to home locations
        log.lh.n = 0 as i32;
        write_head();
    };
}
/// Caller has modified b->data and is done with the buffer.
/// Record the block number and pin in the cache by increasing refcnt.
/// commit()/write_log() will do the disk write.
///
/// log_write() replaces bwrite(); a typical use is:
///   bp = bread(...)
///   modify bp->data[]
///   log_write(bp)
///   brelse(bp)
#[no_mangle]
pub unsafe extern "C" fn log_write(mut b: *mut Buf) {
    let mut i: i32 = 0;
    if log.lh.n >= LOGSIZE || log.lh.n >= log.size - 1 as i32 {
        panic(
            b"too big a transaction\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        );
    }
    if log.outstanding < 1 as i32 {
        panic(
            b"log_write outside of trans\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    acquire(&mut log.lock);
    i = 0;
    while i < log.lh.n {
        if log.lh.block[i as usize] as u32 == (*b).blockno {
            break;
        }
        i += 1
    }
    log.lh.block[i as usize] = (*b).blockno as i32;
    if i == log.lh.n {
        // Add new block to log?
        bpin(b);
        log.lh.n += 1
    }
    release(&mut log.lock);
}
