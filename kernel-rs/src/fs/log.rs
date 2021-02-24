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
use core::mem;

use arrayvec::ArrayVec;
use itertools::*;
use static_assertions::const_assert;

use crate::{
    bio::BufData,
    bio::{Buf, BufUnlocked},
    kernel::kernel_builder,
    lock::{OwnedLock, Sleepablelock},
    param::{BSIZE, LOGSIZE, MAXOPBLOCKS},
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
    bufs: ArrayVec<[BufUnlocked<'static>; LOGSIZE]>,
}

/// Contents of the header block, used for the on-disk header block.
struct LogHeader {
    n: u32,
    block: [u32; LOGSIZE],
}

impl Log {
    pub fn new(dev: u32, start: i32, size: i32) -> Self {
        let mut log = Self {
            dev,
            start,
            size,
            outstanding: 0,
            committing: false,
            bufs: ArrayVec::new(),
        };
        log.recover_from_log();
        log
    }

    /// Copy committed blocks from log to their home location.
    fn install_trans(&mut self) {
        for (tail, dbuf) in self.bufs.drain(..).enumerate() {
            // Read log block.
            let lbuf = kernel_builder()
                .file_system
                .disk
                .read(self.dev, (self.start + tail as i32 + 1) as u32);

            // Read dst.
            let mut dbuf = dbuf.lock();

            // Copy block to dst.
            dbuf.deref_inner_mut()
                .data
                .copy_from_slice(&lbuf.deref_inner().data[..]);

            // Write dst to disk.
            kernel_builder().file_system.disk.write(&mut dbuf);
        }
    }

    /// Read the log header from disk into the in-memory log header.
    fn read_head(&mut self) {
        let mut buf = kernel_builder()
            .file_system
            .disk
            .read(self.dev, self.start as u32);

        const_assert!(mem::size_of::<LogHeader>() <= BSIZE);
        const_assert!(mem::align_of::<BufData>() % mem::align_of::<LogHeader>() == 0);
        // It is safe becuase
        // * buf.data is larger than LogHeader
        // * buf.data is aligned properly.
        // * LogHeader contains only u32's, so does not have any requirements.
        // * buf is locked, so we can access it exclusively.
        let lh = unsafe { &mut *(buf.deref_inner_mut().data.as_mut_ptr() as *mut LogHeader) };

        for b in &lh.block[0..lh.n as usize] {
            self.bufs.push(
                kernel_builder()
                    .file_system
                    .disk
                    .read(self.dev, *b)
                    .unlock(),
            )
        }
    }

    /// Write in-memory log header to disk.
    /// This is the true point at which the
    /// current transaction commits.
    fn write_head(&mut self) {
        let mut buf = kernel_builder()
            .file_system
            .disk
            .read(self.dev, self.start as u32);

        const_assert!(mem::size_of::<LogHeader>() <= BSIZE);
        const_assert!(mem::align_of::<BufData>() % mem::align_of::<LogHeader>() == 0);
        // It is safe becuase
        // * buf.data is larger than LogHeader
        // * buf.data is aligned properly.
        // * LogHeader contains only u32's, so does not have any requirements.
        // * buf is locked, so we can access it exclusively.
        let mut lh = unsafe { &mut *(buf.deref_inner_mut().data.as_mut_ptr() as *mut LogHeader) };

        lh.n = self.bufs.len() as u32;
        for (db, b) in izip!(&mut lh.block, &self.bufs) {
            *db = b.blockno;
        }
        kernel_builder().file_system.disk.write(&mut buf)
    }

    fn recover_from_log(&mut self) {
        self.read_head();

        // If committed, copy from log to disk.
        self.install_trans();

        // Clear the log.
        self.write_head();
    }

    /// Called at the start of each FS system call.
    pub fn begin_op(this: &Sleepablelock<Self>) {
        let mut guard = this.lock();
        loop {
            if guard.committing ||
            // This op might exhaust log space; wait for commit.
            guard.bufs.len() as i32 + (guard.outstanding + 1) * MAXOPBLOCKS as i32 > LOGSIZE as i32
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
    pub fn end_op(this: &Sleepablelock<Self>) {
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
            // It is safe because other threads neither read nor write this log
            // when guard.committing is true.
            unsafe { (*this.get_mut_raw()).commit() };
            let mut guard = this.lock();
            guard.committing = false;
            guard.wakeup();
        };
    }

    /// Copy modified blocks from cache to self.
    fn write_log(&mut self) {
        for (tail, from) in self.bufs.iter().enumerate() {
            // Log block.
            let mut to = kernel_builder()
                .file_system
                .disk
                .read(self.dev, (self.start + tail as i32 + 1) as u32);

            // Cache block.
            let from = kernel_builder()
                .file_system
                .disk
                .read(self.dev, from.blockno);

            to.deref_inner_mut()
                .data
                .copy_from_slice(&from.deref_inner().data[..]);

            // Write the log.
            kernel_builder().file_system.disk.write(&mut to);
        }
    }

    fn commit(&mut self) {
        if !self.bufs.is_empty() {
            // Write modified blocks from cache to self.
            self.write_log();

            // Write header to disk -- the real commit.
            self.write_head();

            // Now install writes to home locations.
            self.install_trans();

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
            !(self.bufs.len() >= LOGSIZE || self.bufs.len() as i32 >= self.size - 1),
            "too big a transaction"
        );
        assert!(self.outstanding >= 1, "write outside of trans");

        for buf in &self.bufs {
            // Log absorbtion.
            if buf.blockno == b.blockno {
                return;
            }
        }

        // Add new block to log?
        self.bufs.push(b.unlock());
    }
}
