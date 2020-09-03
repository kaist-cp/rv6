//! Simple logging that allows concurrent FS system calls.
//!
//! A LOG transaction contains the updates of multiple FS system
//! calls. The logging system only commits when there are
//! no FS system calls active. Thus there is never
//! any reasoning required about whether a commit might
//! write an uncommitted system call's updates to disk.
//!
//! A system call should call begin_op()/end_op() to mark
//! its start and end. Usually begin_op() just increments
//! the count of in-progress FS system calls and returns.
//! But if it thinks the LOG is close to running out, it
//! sleeps until the last outstanding end_op() commits.
//!
//! The LOG is a physical re-do LOG containing disk blocks.
//! The on-disk LOG format:
//!   header block, containing block #s for block A, B, C, ...
//!   block A
//!   block B
//!   block C
//!   ...
//! Log appends are synchronous.
use crate::libc;
use crate::{
    buf::{bread, Buf},
    fs::{Superblock, BSIZE},
    param::{LOGSIZE, MAXOPBLOCKS},
    proc::{sleep, wakeup},
    spinlock::RawSpinlock,
};
use core::ptr;

struct Log {
    lock: RawSpinlock,
    start: i32,
    size: i32,

    /// how many FS sys calls are executing.
    outstanding: i32,

    /// in commit(), please wait.
    committing: i32,
    dev: i32,
    lh: LogHeader,
}

/// Contents of the header block, used for both the on-disk header block
/// and to keep track in memory of logged block# before commit.
#[derive(Copy, Clone)]
struct LogHeader {
    n: i32,
    block: [i32; LOGSIZE],
}

impl Log {
    // TODO: transient measure
    const fn zeroed() -> Self {
        Self {
            lock: RawSpinlock::zeroed(),
            start: 0,
            size: 0,
            outstanding: 0,
            committing: 0,
            dev: 0,
            lh: LogHeader {
                n: 0,
                block: [0; LOGSIZE],
            },
        }
    }
}

static mut LOG: Log = Log::zeroed();

impl Superblock {
    pub unsafe fn initlog(&mut self, dev: i32) {
        if ::core::mem::size_of::<LogHeader>() >= BSIZE {
            panic!("initlog: too big LogHeader");
        }
        LOG.lock.initlock("LOG");
        LOG.start = (*self).logstart as i32;
        LOG.size = (*self).nlog as i32;
        LOG.dev = dev;
        recover_from_log();
    }
}

/// Copy committed blocks from log to their home location
unsafe fn install_trans() {
    for tail in 0..LOG.lh.n {
        // read log block
        let lbuf: *mut Buf = bread(LOG.dev as u32, (LOG.start + tail + 1) as u32);

        // read dst
        let dbuf: *mut Buf = bread(LOG.dev as u32, LOG.lh.block[tail as usize] as u32);

        // copy block to dst
        ptr::copy(
            (*lbuf).data.as_mut_ptr() as *const libc::CVoid,
            (*dbuf).data.as_mut_ptr() as *mut libc::CVoid,
            BSIZE,
        );

        // write dst to disk
        (*dbuf).write();
        (*dbuf).unpin();
        (*lbuf).release();
        (*dbuf).release();
    }
}

/// Read the log header from disk into the in-memory log header
unsafe fn read_head() {
    let buf: *mut Buf = bread(LOG.dev as u32, LOG.start as u32);
    let lh: *mut LogHeader = (*buf).data.as_mut_ptr() as *mut LogHeader;
    LOG.lh.n = (*lh).n;
    for i in 0..LOG.lh.n {
        LOG.lh.block[i as usize] = (*lh).block[i as usize];
    }
    (*buf).release();
}

/// Write in-memory log header to disk.
/// This is the true point at which the
/// current transaction commits.
unsafe fn write_head() {
    let buf: *mut Buf = bread(LOG.dev as u32, LOG.start as u32);
    let mut hb: *mut LogHeader = (*buf).data.as_mut_ptr() as *mut LogHeader;
    (*hb).n = LOG.lh.n;
    for i in 0..LOG.lh.n {
        (*hb).block[i as usize] = LOG.lh.block[i as usize];
    }
    (*buf).write();
    (*buf).release();
}

unsafe fn recover_from_log() {
    read_head();

    // if committed, copy from log to disk
    install_trans();
    LOG.lh.n = 0;

    // clear the log
    write_head();
}

/// called at the start of each FS system call.
pub unsafe fn begin_op() {
    LOG.lock.acquire();
    loop {
        if LOG.committing != 0 ||
            // this op might exhaust log space; wait for commit.
            LOG.lh.n + (LOG.outstanding + 1) * MAXOPBLOCKS as i32 > LOGSIZE as i32
        {
            sleep(&mut LOG as *mut Log as *mut libc::CVoid, &mut LOG.lock);
        } else {
            LOG.outstanding += 1;
            LOG.lock.release();
            break;
        }
    }
}

/// called at the end of each FS system call.
/// commits if this was the last outstanding operation.
pub unsafe fn end_op() {
    let mut do_commit: i32 = 0;
    LOG.lock.acquire();
    LOG.outstanding -= 1;
    if LOG.committing != 0 {
        panic!("LOG.committing");
    }
    if LOG.outstanding == 0 {
        do_commit = 1;
        LOG.committing = 1
    } else {
        // begin_op() may be waiting for LOG space,
        // and decrementing log.outstanding has decreased
        // the amount of reserved space.
        wakeup(&mut LOG as *mut Log as *mut libc::CVoid);
    }
    LOG.lock.release();
    if do_commit != 0 {
        // call commit w/o holding locks, since not allowed
        // to sleep with locks.
        commit();
        LOG.lock.acquire();
        LOG.committing = 0;
        wakeup(&mut LOG as *mut Log as *mut libc::CVoid);
        LOG.lock.release();
    };
}

/// Copy modified blocks from cache to LOG.
unsafe fn write_log() {
    for tail in 0..LOG.lh.n {
        // log block
        let to: *mut Buf = bread(LOG.dev as u32, (LOG.start + tail + 1) as u32);

        // cache block
        let from: *mut Buf = bread(LOG.dev as u32, LOG.lh.block[tail as usize] as u32);

        ptr::copy(
            (*from).data.as_mut_ptr() as *const libc::CVoid,
            (*to).data.as_mut_ptr() as *mut libc::CVoid,
            BSIZE,
        );

        // write the log
        (*to).write();
        (*from).release();
        (*to).release();
    }
}

unsafe fn commit() {
    if LOG.lh.n > 0 {
        // Write modified blocks from cache to LOG
        write_log();

        // Write header to disk -- the real commit
        write_head();

        // Now install writes to home locations
        install_trans();
        LOG.lh.n = 0;

        // Erase the transaction from the LOG
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
pub unsafe fn log_write(b: *mut Buf) {
    if LOG.lh.n >= LOGSIZE as i32 || LOG.lh.n >= LOG.size as i32 - 1 {
        panic!("too big a transaction");
    }
    if LOG.outstanding < 1 {
        panic!("log_write outside of trans");
    }
    LOG.lock.acquire();
    let mut absorbed = false;
    for i in 0..LOG.lh.n {
        // log absorbtion
        if LOG.lh.block[i as usize] as u32 == (*b).blockno {
            LOG.lh.block[i as usize] = (*b).blockno as i32;
            absorbed = true;
            break;
        }
    }

    // Add new block to log?
    if !absorbed {
        LOG.lh.block[LOG.lh.n as usize] = (*b).blockno as i32;
        (*b).pin();
        LOG.lh.n += 1;
    }
    LOG.lock.release();
}
