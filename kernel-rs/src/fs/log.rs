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
    param::{BSIZE, LOGSIZE, MAXOPBLOCKS},
    sleepablelock::Sleepablelock,
    virtio_disk::Disk,
};

use super::Superblock;

pub struct Log {
    start: i32,
    size: i32,

    /// How many FS sys calls are executing?
    outstanding: i32,

    /// In commit(), please wait.
    committing: bool,
    dev: u32,
    lh: LogHeaderInMemory,
}

/// Contents of the header block, used for both the on-disk header block
/// and to keep track in memory of logged block# before commit.
struct LogHeader {
    n: i32,
    block: [i32; LOGSIZE],
}

/// Contents of the header block, used for both the on-disk header block
/// and to keep track in memory of logged block# before commit.
struct LogHeaderInMemory {
    block: ArrayVec<[BufUnlocked; LOGSIZE]>,
}

impl Log {
    pub fn new(dev: u32, superblock: &Superblock) -> Self {
        assert!(
            mem::size_of::<LogHeader>() < BSIZE,
            "Log::new: too big LogHeader"
        );

        let mut log = Self {
            start: superblock.logstart as i32,
            size: superblock.nlog as i32,
            outstanding: 0,
            committing: false,
            dev,
            lh: LogHeaderInMemory {
                block: ArrayVec::new(),
            },
        };
        unsafe {
            log.recover_from_log();
        }

        log
    }

    /// Copy committed blocks from log to their home location.
    unsafe fn install_trans(&mut self) {
        for (tail, dbuf) in self.lh.block.drain(..).enumerate() {
            // Read log block.
            let lbuf = Disk::read(self.dev as u32, (self.start + tail as i32 + 1) as u32);

            // Read dst.
            let mut dbuf = dbuf.lock();

            // Copy block to dst.
            ptr::copy(
                lbuf.deref_inner().data.as_ptr(),
                dbuf.deref_mut_inner().data.as_mut_ptr(),
                BSIZE,
            );

            // Write dst to disk.
            Disk::write(&mut dbuf)
        }
    }

    /// Read the log header from disk into the in-memory log header.
    unsafe fn read_head(&mut self) {
        let mut buf = Disk::read(self.dev as u32, self.start as u32);
        let lh = buf.deref_mut_inner().data.as_mut_ptr() as *mut LogHeader;
        for b in &(*lh).block[0..(*lh).n as usize] {
            self.lh
                .block
                .push(BufUnlocked::unforget(self.dev as u32, *b as u32).unwrap());
        }
    }

    /// Write in-memory log header to disk.
    /// This is the true point at which the
    /// current transaction commits.
    unsafe fn write_head(&mut self) {
        let mut buf = Disk::read(self.dev as u32, self.start as u32);
        let mut hb = buf.deref_mut_inner().data.as_mut_ptr() as *mut LogHeader;
        (*hb).n = self.lh.block.len() as i32;
        for (i, b) in self.lh.block.iter().enumerate() {
            (*hb).block[i as usize] = b.blockno as i32;
        }
        Disk::write(&mut buf)
    }

    unsafe fn recover_from_log(&mut self) {
        self.read_head();

        // If committed, copy from log to disk.
        self.install_trans();

        // Clear the log.
        self.write_head();
    }

    /// Called at the start of each FS system call.
    pub unsafe fn begin_op(this: &Sleepablelock<Self>) {
        let mut guard = this.lock();
        loop {
            if guard.committing ||
            // This op might exhaust log space; wait for commit.
            guard.lh.block.len() as i32 + (guard.outstanding + 1) * MAXOPBLOCKS as i32 > LOGSIZE as i32
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
        for (tail, from) in self.lh.block.iter().enumerate() {
            // Log block.
            let mut to = Disk::read(self.dev as u32, (self.start + tail as i32 + 1) as u32);

            // Cache block.
            let from = Disk::read(self.dev as u32, from.blockno);

            ptr::copy(
                from.deref_inner().data.as_ptr(),
                to.deref_mut_inner().data.as_mut_ptr(),
                BSIZE,
            );

            // Write the log.
            Disk::write(&mut to)
        }
    }

    unsafe fn commit(&mut self) {
        if !self.lh.block.is_empty() {
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
    pub unsafe fn write(&mut self, b: Buf) {
        assert!(
            !(self.lh.block.len() >= LOGSIZE || self.lh.block.len() as i32 >= self.size - 1),
            "too big a transaction"
        );
        assert!(self.outstanding >= 1, "write outside of trans");

        for buf in &self.lh.block {
            // Log absorbtion.
            if buf.blockno == (*b).blockno {
                return;
            }
        }

        // Add new block to log?
        self.lh.block.push(b.unlock());
    }
}
