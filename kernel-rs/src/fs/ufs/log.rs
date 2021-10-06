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
    bio::{Buf, BufData, BufUnlocked},
    hal::hal,
    lock::SleepableLock,
    param::{BSIZE, LOGSIZE, MAXOPBLOCKS},
    proc::KernelCtx,
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
    bufs: ArrayVec<BufUnlocked, LOGSIZE>,
}

/// Contents of the header block, used for the on-disk header block.
struct LogHeader {
    n: u32,
    block: [u32; LOGSIZE],
}

impl Log {
    pub fn new(dev: u32, start: i32, size: i32, ctx: &KernelCtx<'_, '_>) -> Self {
        let mut log = Self {
            dev,
            start,
            size,
            outstanding: 0,
            committing: false,
            bufs: ArrayVec::new(),
        };
        log.recover_from_log(ctx);
        log
    }

    /// Copy committed blocks from log to their home location.
    fn install_trans(&mut self, ctx: &KernelCtx<'_, '_>) {
        let dev = self.dev;
        let start = self.start;

        for (tail, dbuf) in self.bufs.drain(..).enumerate() {
            // Read log block.
            let lbuf = hal()
                .disk()
                .read(dev, (start + tail as i32 + 1) as u32, ctx);

            // Read dst.
            let mut dbuf = dbuf.lock(ctx);

            // Copy block to dst.
            dbuf.deref_inner_mut()
                .data
                .copy_from(&lbuf.deref_inner().data);

            // Write dst to disk.
            hal().disk().write(&mut dbuf, ctx);

            lbuf.free(ctx);
            dbuf.free(ctx);
        }
    }

    /// Read the log header from disk into the in-memory log header.
    fn read_head(&mut self, ctx: &KernelCtx<'_, '_>) {
        let mut buf = hal().disk().read(self.dev, self.start as u32, ctx);

        const_assert!(mem::size_of::<LogHeader>() <= BSIZE);
        const_assert!(mem::align_of::<BufData>() % mem::align_of::<LogHeader>() == 0);
        // SAFETY:
        // * buf.data is larger than LogHeader
        // * buf.data is aligned properly.
        // * LogHeader contains only u32's, so does not have any requirements.
        // * buf is locked, so we can access it exclusively.
        let lh = unsafe { &mut *(buf.deref_inner_mut().data.as_mut_ptr() as *mut LogHeader) };
        buf.free(ctx);

        for b in &lh.block[0..lh.n as usize] {
            let buf = hal().disk().read(self.dev, *b, ctx).unlock(ctx);
            self.bufs.push(buf);
        }
    }

    /// Write in-memory log header to disk.
    /// This is the true point at which the
    /// current transaction commits.
    fn write_head(&mut self, ctx: &KernelCtx<'_, '_>) {
        let mut buf = hal().disk().read(self.dev, self.start as u32, ctx);

        const_assert!(mem::size_of::<LogHeader>() <= BSIZE);
        const_assert!(mem::align_of::<BufData>() % mem::align_of::<LogHeader>() == 0);
        // SAFETY:
        // * buf.data is larger than LogHeader
        // * buf.data is aligned properly.
        // * LogHeader contains only u32's, so does not have any requirements.
        // * buf is locked, so we can access it exclusively.
        let mut lh = unsafe { &mut *(buf.deref_inner_mut().data.as_mut_ptr() as *mut LogHeader) };

        lh.n = self.bufs.len() as u32;
        for (db, b) in izip!(&mut lh.block, &self.bufs) {
            *db = b.blockno;
        }
        hal().disk().write(&mut buf, ctx);
        buf.free(ctx);
    }

    fn recover_from_log(&mut self, ctx: &KernelCtx<'_, '_>) {
        self.read_head(ctx);

        // If committed, copy from log to disk.
        self.install_trans(ctx);

        // Clear the log.
        self.write_head(ctx);
    }

    /// Copy modified blocks from cache to self.
    fn write_log(&mut self, ctx: &KernelCtx<'_, '_>) {
        for (tail, from) in self.bufs.iter().enumerate() {
            // Log block.
            let mut to = hal()
                .disk()
                .read(self.dev, (self.start + tail as i32 + 1) as u32, ctx);

            // Cache block.
            let from = hal().disk().read(self.dev, from.blockno, ctx);

            to.deref_inner_mut()
                .data
                .copy_from(&from.deref_inner().data);

            // Write the log.
            hal().disk().write(&mut to, ctx);

            to.free(ctx);
            from.free(ctx);
        }
    }

    fn commit(&mut self, ctx: &KernelCtx<'_, '_>) {
        if !self.bufs.is_empty() {
            // Write modified blocks from cache to self.
            self.write_log(ctx);

            // Write header to disk -- the real commit.
            self.write_head(ctx);

            // Now install writes to home locations.
            self.install_trans(ctx);

            // Erase the transaction from the self.
            self.write_head(ctx);
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
    pub fn write(&mut self, b: Buf, ctx: &KernelCtx<'_, '_>) {
        assert!(
            !(self.bufs.len() >= LOGSIZE || self.bufs.len() as i32 >= self.size - 1),
            "too big a transaction"
        );
        assert!(self.outstanding >= 1, "write outside of trans");

        if self.bufs.iter().all(|buf| buf.blockno != b.blockno) {
            // Add new block to log
            self.bufs.push(b.unlock(ctx));
        } else {
            b.free(ctx);
        }
    }
}

impl SleepableLock<Log> {
    /// Called at the start of each FS system call.
    pub fn begin_op(&self, ctx: &KernelCtx<'_, '_>) {
        let mut guard = self.lock();
        loop {
            if guard.committing ||
            // This op might exhaust log space; wait for commit.
            guard.bufs.len() as i32 + (guard.outstanding + 1) * MAXOPBLOCKS as i32 > LOGSIZE as i32
            {
                guard.sleep(ctx);
            } else {
                guard.outstanding += 1;
                break;
            }
        }
    }

    /// Called at the end of each FS system call.
    /// Commits if this was the last outstanding operation.
    pub fn end_op(&self, ctx: &KernelCtx<'_, '_>) {
        let mut guard = self.lock();
        guard.outstanding -= 1;
        assert!(!guard.committing, "guard.committing");

        if guard.outstanding == 0 {
            // Since outstanding is 0, no ongoing transaction exists.
            // The lock is still held, so new transactions cannot start.
            guard.committing = true;
            // Committing is true, so new transactions cannot start even after releasing the lock.

            // Call commit w/o holding locks, since not allowed to sleep with locks.
            guard.reacquire_after(||
                // SAFETY: there is no another transaction, so `inner` cannot be read or written.
                unsafe { &mut *self.get_mut_raw() }.commit(ctx));

            guard.committing = false;
        }

        // begin_op() may be waiting for LOG space, and decrementing log.outstanding has decreased
        // the amount of reserved space.
        guard.wakeup(ctx.kernel());
    }
}
