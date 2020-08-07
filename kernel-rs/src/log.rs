use crate::libc;
use crate::{
    bio::bread,
    buf::Buf,
    fs::{Superblock, BSIZE},
    param::{LOGSIZE, MAXOPBLOCKS},
    printf::panic,
    proc::{sleep, wakeup},
    spinlock::Spinlock,
};
use core::ptr;

#[derive(Copy, Clone)]
struct Log {
    lock: Spinlock,
    start: i32,
    size: i32,

    /// how many FS sys calls are executing.
    outstanding: i32,

    /// in commit(), please wait.
    committing: i32,
    dev: i32,
    lh: LogHeader,
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
struct LogHeader {
    n: i32,
    block: [i32; LOGSIZE as usize],
}

impl Log {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            lock: Spinlock::zeroed(),
            start: 0,
            size: 0,
            outstanding: 0,
            committing: 0,
            dev: 0,
            lh: LogHeader {
                n: 0,
                block: [0; LOGSIZE as usize],
            },
        }
    }
}

static mut log: Log = Log::zeroed();

impl Superblock {
    pub unsafe fn initlog(&mut self, mut dev: i32) {
        if ::core::mem::size_of::<LogHeader>() >= BSIZE as usize {
            panic(
                b"initlog: too big LogHeader\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
            );
        }
        log.lock
            .initlock(b"log\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        log.start = (*self).logstart as i32;
        log.size = (*self).nlog as i32;
        log.dev = dev;
        recover_from_log();
    }
}

/// Copy committed blocks from log to their home location
unsafe fn install_trans() {
    for tail in 0..log.lh.n {
        let mut lbuf: *mut Buf = bread(log.dev as u32, (log.start + tail + 1) as u32);

        // read dst
        let mut dbuf: *mut Buf = bread(log.dev as u32, log.lh.block[tail as usize] as u32);

        // copy block to dst
        ptr::copy(
            (*lbuf).data.as_mut_ptr() as *const libc::c_void,
            (*dbuf).data.as_mut_ptr() as *mut libc::c_void,
            BSIZE as usize,
        );

        // write dst to disk
        (*dbuf).write();
        (*dbuf).unpin();
        (*lbuf).release();
        (*dbuf).release();
        // tail += 1
    }
}

/// Read the log header from disk into the in-memory log header
unsafe fn read_head() {
    let mut buf: *mut Buf = bread(log.dev as u32, log.start as u32);
    let mut lh: *mut LogHeader = (*buf).data.as_mut_ptr() as *mut LogHeader;
    log.lh.n = (*lh).n;
    for i in 0..log.lh.n {
        log.lh.block[i as usize] = (*lh).block[i as usize];
    }
    (*buf).release();
}

/// Write in-memory log header to disk.
/// This is the true point at which the
/// current transaction commits.
unsafe fn write_head() {
    let mut buf: *mut Buf = bread(log.dev as u32, log.start as u32);
    let mut hb: *mut LogHeader = (*buf).data.as_mut_ptr() as *mut LogHeader;
    (*hb).n = log.lh.n;
    for i in 0..log.lh.n {
        (*hb).block[i as usize] = log.lh.block[i as usize];
    }
    (*buf).write();
    (*buf).release();
}

unsafe fn recover_from_log() {
    read_head();

    // if committed, copy from log to disk
    install_trans();
    log.lh.n = 0;

    // clear the log
    write_head();
}

/// called at the start of each FS system call.
pub unsafe fn begin_op() {
    log.lock.acquire();
    loop {
        if log.committing != 0 ||
            // this op might exhaust log space; wait for commit.
            log.lh.n + (log.outstanding + 1) * MAXOPBLOCKS > LOGSIZE
        {
            sleep(&mut log as *mut Log as *mut libc::c_void, &mut log.lock);
        } else {
            log.outstanding += 1;
            log.lock.release();
            break;
        }
    }
}

/// called at the end of each FS system call.
/// commits if this was the last outstanding operation.
pub unsafe fn end_op() {
    let mut do_commit: i32 = 0;
    log.lock.acquire();
    log.outstanding -= 1;
    if log.committing != 0 {
        panic(b"log.committing\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if log.outstanding == 0 {
        do_commit = 1;
        log.committing = 1
    } else {
        // begin_op() may be waiting for log space,
        // and decrementing log.outstanding has decreased
        // the amount of reserved space.
        wakeup(&mut log as *mut Log as *mut libc::c_void);
    }
    log.lock.release();
    if do_commit != 0 {
        // call commit w/o holding locks, since not allowed
        // to sleep with locks.
        commit();
        log.lock.acquire();
        log.committing = 0;
        wakeup(&mut log as *mut Log as *mut libc::c_void);
        log.lock.release();
    };
}

/// Copy modified blocks from cache to log.
unsafe fn write_log() {
    for tail in 0..log.lh.n {
        // log block
        let mut to: *mut Buf = bread(log.dev as u32, (log.start + tail + 1) as u32);

        // cache block
        let mut from: *mut Buf = bread(log.dev as u32, log.lh.block[tail as usize] as u32);

        ptr::copy(
            (*from).data.as_mut_ptr() as *const libc::c_void,
            (*to).data.as_mut_ptr() as *mut libc::c_void,
            BSIZE as usize,
        );

        // write the log
        (*to).write();
        (*from).release();
        (*to).release();
    }
}

unsafe fn commit() {
    if log.lh.n > 0 {
        // Write modified blocks from cache to log
        write_log();

        // Write header to disk -- the real commit
        write_head();

        // Now install writes to home locations
        install_trans();
        log.lh.n = 0;

        // Erase the transaction from the log
        write_head();
    };
}

/// Caller has modified b->data and is done with the buffer.
/// Record the block number and pin in the cache by increasing refcnt.
/// commit()/write_log() will do the disk write.
///
/// log_write() replaces write(); a typical use is:
///   bp = bread(...)
///   modify bp->data[]
///   log_write(bp)
///   (*bp).release()
pub unsafe fn log_write(mut b: *mut Buf) {
    if log.lh.n >= LOGSIZE || log.lh.n >= log.size - 1 {
        panic(
            b"too big a transaction\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        );
    }
    if log.outstanding < 1 {
        panic(
            b"log_write outside of trans\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    log.lock.acquire();
    let mut absorbed = false;
    for i in 0..log.lh.n {
        // log absorbtion
        if log.lh.block[i as usize] as u32 == (*b).blockno {
            log.lh.block[i as usize] = (*b).blockno as i32;
            absorbed = true;
            break;
        }
    }

    // Add new block to log?
    if !absorbed {
        log.lh.block[log.lh.n as usize] = (*b).blockno as i32;
        (*b).pin();
        log.lh.n += 1;
    }
    log.lock.release();
}
