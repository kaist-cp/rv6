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
use arrayvec::ArrayVec;
use core::{mem, ptr};

use crate::{
    bio::{Buf, BufUnlocked},
    kernel::kernel,
    param::{BSIZE, LOGSIZE, MAXOPBLOCKS},
    sleepablelock::Sleepablelock,
};

pub struct Log {
    dev: u32,
    start: i32,
    size: i32,

    /// How many FS sys calls are executing?
    outstanding: i32,

    /// In commit(), please wait.
    committing: bool,

    /// Contents of the header block, used to keep track in memory of logged block# before commit.
    lh: ArrayVec<[BufUnlocked<'static>; LOGSIZE]>,
}

/// Contents of the header block, used for the on-disk header block.
struct LogHeader {
    n: u32,
    block: [u32; LOGSIZE],
}

// `LogHeader` must be fit in a block.
const_assert!(mem::size_of::<LogHeader>() < BSIZE);

impl Log {
    pub fn new(dev: u32, start: i32, size: i32) -> Self {
        let mut log = Self {
            dev,
            start,
            size,
            outstanding: 0,
            committing: false,
            lh: ArrayVec::new(),
        };
        unsafe {
            log.recover_from_log();
        }

        log
    }

    /// Copy committed blocks from log to their home location.
    unsafe fn install_trans(&mut self, recovering: bool) {
        for (tail, dbuf) in self.lh.drain(..).enumerate() {
            // Read log block.
            let lbuf = kernel()
                .file_system
                .disk
                .read(self.dev as u32, (self.start + tail as i32 + 1) as u32);

            // Read dst.
            let mut dbuf = dbuf.lock();

            // Copy block to dst.
            ptr::copy(
                lbuf.deref_inner().data.as_ptr(),
                dbuf.deref_mut_inner().data.as_mut_ptr(),
                BSIZE,
            );

            // Write dst to disk.
            kernel().file_system.disk.write(&mut dbuf);

            if recovering {
                mem::forget(dbuf);
            }
        }
    }

    /// Read the log header from disk into the in-memory log header.
    unsafe fn read_head(&mut self) {
        let mut buf = kernel()
            .file_system
            .disk
            .read(self.dev as u32, self.start as u32);
        let lh = buf.deref_mut_inner().data.as_mut_ptr() as *mut LogHeader;
        for b in &(*lh).block[0..(*lh).n as usize] {
            self.lh.push(
                kernel()
                    .bcache
                    .buf_unforget(self.dev as u32, *b as u32)
                    .unwrap(),
            );
        }
    }

    /// Write in-memory log header to disk.
    /// This is the true point at which the
    /// current transaction commits.
    unsafe fn write_head(&mut self) {
        let mut buf = kernel()
            .file_system
            .disk
            .read(self.dev as u32, self.start as u32);
        let mut hb = &mut *(buf.deref_mut_inner().data.as_mut_ptr() as *mut LogHeader);
        hb.n = self.lh.len() as u32;
        for (db, b) in izip!(&mut hb.block, &self.lh) {
            *db = (*b).blockno;
        }
        kernel().file_system.disk.write(&mut buf)
    }

    unsafe fn recover_from_log(&mut self) {
        self.read_head();

        // If committed, copy from log to disk.
        self.install_trans(true);

        // Clear the log.
        self.write_head();
    }

    /// Called at the start of each FS system call.
    pub fn begin_op(this: &Sleepablelock<Self>) {
        let mut guard = this.lock();
        loop {
            if guard.committing ||
            // This op might exhaust log space; wait for commit.
            guard.lh.len() as i32 + (guard.outstanding + 1) * MAXOPBLOCKS as i32 > LOGSIZE as i32
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
        let mut guard = this.lock();
        guard.outstanding -= 1;
        assert!(!guard.committing, "guard.committing");

        let do_commit = if guard.outstanding == 0 {
            guard.committing = true;
            true
        } else {
            // begin_op() may be waiting for LOG space,
            // and decrementing log.outstanding has decreased
            // the amount of reserved space.
            guard.wakeup();
            false
        };
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
    unsafe fn write_log(&mut self) {
        for (tail, from) in self.lh.iter().enumerate() {
            // Log block.
            let mut to = kernel()
                .file_system
                .disk
                .read(self.dev as u32, (self.start + tail as i32 + 1) as u32);

            // Cache block.
            let from = kernel()
                .file_system
                .disk
                .read(self.dev as u32, from.blockno);

            ptr::copy(
                from.deref_inner().data.as_ptr(),
                to.deref_mut_inner().data.as_mut_ptr(),
                BSIZE,
            );

            // Write the log.
            kernel().file_system.disk.write(&mut to)
        }
    }

    unsafe fn commit(&mut self) {
        if !self.lh.is_empty() {
            // Write modified blocks from cache to self.
            self.write_log();

            // Write header to disk -- the real commit.
            self.write_head();

            // Now install writes to home locations.
            self.install_trans(false);

            // Erase the transaction from the self.
            self.write_head();
        };
    }

    /// Caller has modified b->data and is done with the buffer.
    /// Record the block number and pin in the cache by increasing refcnt.
    /// commit()/write_log() will do the disk write.
    ///
    /// write() replaces write(); a typical use is:
    ///   bp = Disk::read(...)
    ///   modify bp->data[]
    ///   write(bp)
    pub fn write(&mut self, b: Buf<'static>) {
        assert!(
            !(self.lh.len() >= LOGSIZE || self.lh.len() as i32 >= self.size - 1),
            "too big a transaction"
        );
        assert!(self.outstanding >= 1, "write outside of trans");

        for buf in &self.lh {
            // Log absorbtion.
            if buf.blockno == (*b).blockno {
                return;
            }
        }

        // Add new block to log?
        self.lh.push(b.unlock());
    }
}
