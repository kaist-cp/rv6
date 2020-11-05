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
use crate::{
    bio::Buf,
    fs::{Superblock, BSIZE},
    param::{LOGSIZE, MAXOPBLOCKS},
    sleepablelock::Sleepablelock,
};
use core::ptr;

pub struct Log {
    start: i32,
    size: i32,

    /// How many FS sys calls are executing?
    outstanding: i32,

    /// In commit(), please wait.
    committing: bool,
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
    pub fn new(dev: i32, superblock: &Superblock) -> Self {
        if ::core::mem::size_of::<LogHeader>() >= BSIZE {
            panic!("Log::new: too big LogHeader");
        }

        let mut log = Self {
            start: superblock.logstart as i32,
            size: superblock.nlog as i32,
            outstanding: 0,
            committing: false,
            dev,
            lh: LogHeader {
                n: 0,
                block: [0; LOGSIZE],
            },
        };
        unsafe {
            log.recover_from_log();
        }

        log
    }

    /// Copy committed blocks from log to their home location.
    unsafe fn install_trans(&self) {
        for tail in 0..self.lh.n {
            // Read log block.
            let mut lbuf = Buf::new(self.dev as u32, (self.start + tail + 1) as u32);

            // Read dst.
            let mut dbuf = Buf::new(self.dev as u32, self.lh.block[tail as usize] as u32);

            // Copy block to dst.
            ptr::copy(
                lbuf.deref_mut_inner().data.as_mut_ptr(),
                dbuf.deref_mut_inner().data.as_mut_ptr(),
                BSIZE,
            );

            // Write dst to disk.
            dbuf.write();
            dbuf.unpin();
        }
    }

    /// Read the log header from disk into the in-memory log header.
    unsafe fn read_head(&mut self) {
        let mut buf = Buf::new(self.dev as u32, self.start as u32);
        let lh: *mut LogHeader = buf.deref_mut_inner().data.as_mut_ptr() as *mut LogHeader;
        self.lh.n = (*lh).n;
        for i in 0..self.lh.n {
            self.lh.block[i as usize] = (*lh).block[i as usize];
        }
    }

    /// Write in-memory log header to disk.
    /// This is the true point at which the
    /// current transaction commits.
    unsafe fn write_head(&self) {
        let mut buf = Buf::new(self.dev as u32, self.start as u32);
        let mut hb: *mut LogHeader = buf.deref_mut_inner().data.as_mut_ptr() as *mut LogHeader;
        (*hb).n = self.lh.n;
        for i in 0..self.lh.n {
            (*hb).block[i as usize] = self.lh.block[i as usize];
        }
        buf.write();
    }

    unsafe fn recover_from_log(&mut self) {
        self.read_head();

        // If committed, copy from log to disk.
        self.install_trans();
        self.lh.n = 0;

        // Clear the log.
        self.write_head();
    }

    /// Called at the start of each FS system call.
    pub unsafe fn begin_op(this: &Sleepablelock<Self>) {
        let mut guard = this.lock();
        loop {
            if guard.committing ||
            // This op might exhaust log space; wait for commit.
            guard.lh.n + (guard.outstanding + 1) * MAXOPBLOCKS as i32 > LOGSIZE as i32
            {
                guard.sleep();
            } else {
                guard.outstanding += 1;
                break;
            }
        }
    }

    /// Called at the end of each FS system call.
    /// Commits if this was the last outstanding operation.
    pub unsafe fn end_op(this: &Sleepablelock<Self>) {
        let mut do_commit = false;
        let mut guard = this.lock();
        guard.outstanding -= 1;
        if guard.committing {
            panic!("guard.committing");
        }
        if guard.outstanding == 0 {
            do_commit = true;
            guard.committing = true;
        } else {
            // begin_op() may be waiting for LOG space,
            // and decrementing log.outstanding has decreased
            // the amount of reserved space.
            guard.wakeup();
        }
        drop(guard);
        if do_commit {
            // Call commit w/o holding locks, since not allowed
            // to sleep with locks.
            this.get_mut_unchecked().commit();
            let mut guard = this.lock();
            guard.committing = false;
            guard.wakeup();
        };
    }

    /// Copy modified blocks from cache to self.
    unsafe fn write_log(&self) {
        for tail in 0..self.lh.n {
            // Log block.
            let mut to = Buf::new(self.dev as u32, (self.start + tail + 1) as u32);

            // Cache block.
            let mut from = Buf::new(self.dev as u32, self.lh.block[tail as usize] as u32);

            ptr::copy(
                from.deref_mut_inner().data.as_mut_ptr(),
                to.deref_mut_inner().data.as_mut_ptr(),
                BSIZE,
            );

            // Write the log.
            to.write();
        }
    }

    unsafe fn commit(&mut self) {
        if self.lh.n > 0 {
            // Write modified blocks from cache to self.
            self.write_log();

            // Write header to disk -- the real commit.
            self.write_head();

            // Now install writes to home locations.
            self.install_trans();
            self.lh.n = 0;

            // Erase the transaction from the self.
            self.write_head();
        };
    }

    /// Caller has modified b->data and is done with the buffer.
    /// Record the block number and pin in the cache by increasing refcnt.
    /// commit()/write_log() will do the disk write.
    ///
    /// log_write() replaces write(); a typical use is:
    ///   bp = Buf::new(...)
    ///   modify bp->data[]
    ///   log_write(bp)
    pub unsafe fn log_write(&mut self, b: Buf) {
        if self.lh.n >= LOGSIZE as i32 || self.lh.n >= self.size as i32 - 1 {
            panic!("too big a transaction");
        }
        if self.outstanding < 1 {
            panic!("log_write outside of trans");
        }
        let mut absorbed = false;
        for i in 0..self.lh.n {
            // Log absorbtion.
            if self.lh.block[i as usize] as u32 == (*b).data.blockno {
                self.lh.block[i as usize] = (*b).data.blockno as i32;
                absorbed = true;
                break;
            }
        }

        // Add new block to log?
        if !absorbed {
            self.lh.block[self.lh.n as usize] = (*b).data.blockno as i32;
            b.pin();
            self.lh.n += 1;
        }
    }
}
