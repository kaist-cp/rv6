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
    bio::{bpin, brelease, bunpin},
    buf::Buf,
    fs::{Superblock, BSIZE},
    param::{LOGSIZE, MAXOPBLOCKS},
    proc::WaitChannel,
    spinlock::RawSpinlock,
};
use core::ptr;

pub struct Log {
    lock: RawSpinlock,
    start: i32,
    size: i32,

    /// How many FS sys calls are executing?
    outstanding: i32,

    /// In commit(), please wait.
    committing: i32,
    dev: i32,
    lh: LogHeader,

    /// WaitChannel saying committing is done or there is enough unreserved log space.
    waitchannel: WaitChannel,
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
            lock: RawSpinlock::new("LOG"),
            start: superblock.logstart as i32,
            size: superblock.nlog as i32,
            outstanding: 0,
            committing: 0,
            dev,
            lh: LogHeader {
                n: 0,
                block: [0; LOGSIZE],
            },
            waitchannel: WaitChannel::new(),
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
            let lbuf: *mut Buf = Buf::read(self.dev as u32, (self.start + tail + 1) as u32);

            // Read dst.
            let dbuf: *mut Buf = Buf::read(self.dev as u32, self.lh.block[tail as usize] as u32);

            // Copy block to dst.
            ptr::copy(
                (*lbuf).inner.data.as_mut_ptr(),
                (*dbuf).inner.data.as_mut_ptr(),
                BSIZE,
            );

            // Write dst to disk.
            (*dbuf).write();
            bunpin(&mut *dbuf);
            brelease(&mut *lbuf);
            brelease(&mut *dbuf);
        }
    }

    /// Read the log header from disk into the in-memory log header.
    unsafe fn read_head(&mut self) {
        let buf: *mut Buf = Buf::read(self.dev as u32, self.start as u32);
        let lh: *mut LogHeader = (*buf).inner.data.as_mut_ptr() as *mut LogHeader;
        self.lh.n = (*lh).n;
        for i in 0..self.lh.n {
            self.lh.block[i as usize] = (*lh).block[i as usize];
        }
        brelease(&mut *buf);
    }

    /// Write in-memory log header to disk.
    /// This is the true point at which the
    /// current transaction commits.
    unsafe fn write_head(&self) {
        let buf: *mut Buf = Buf::read(self.dev as u32, self.start as u32);
        let mut hb: *mut LogHeader = (*buf).inner.data.as_mut_ptr() as *mut LogHeader;
        (*hb).n = self.lh.n;
        for i in 0..self.lh.n {
            (*hb).block[i as usize] = self.lh.block[i as usize];
        }
        (*buf).write();
        brelease(&mut *buf);
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
    pub unsafe fn begin_op(&mut self) {
        self.lock.acquire();
        loop {
            if self.committing != 0 ||
            // This op might exhaust log space; wait for commit.
            self.lh.n + (self.outstanding + 1) * MAXOPBLOCKS as i32 > LOGSIZE as i32
            {
                self.waitchannel.sleep(&mut self.lock);
            } else {
                self.outstanding += 1;
                self.lock.release();
                break;
            }
        }
    }

    /// Called at the end of each FS system call.
    /// Commits if this was the last outstanding operation.
    pub unsafe fn end_op(&mut self) {
        let mut do_commit = false;
        self.lock.acquire();
        self.outstanding -= 1;
        if self.committing != 0 {
            panic!("self.committing");
        }
        if self.outstanding == 0 {
            do_commit = true;
            self.committing = 1
        } else {
            // begin_op() may be waiting for LOG space,
            // and decrementing log.outstanding has decreased
            // the amount of reserved space.
            self.waitchannel.wakeup();
        }
        self.lock.release();
        if do_commit {
            // Call commit w/o holding locks, since not allowed
            // to sleep with locks.
            self.commit();
            self.lock.acquire();
            self.committing = 0;
            self.waitchannel.wakeup();
            self.lock.release();
        };
    }

    /// Copy modified blocks from cache to self.
    unsafe fn write_log(&self) {
        for tail in 0..self.lh.n {
            // Log block.
            let to: *mut Buf = Buf::read(self.dev as u32, (self.start + tail + 1) as u32);

            // Cache block.
            let from: *mut Buf = Buf::read(self.dev as u32, self.lh.block[tail as usize] as u32);

            ptr::copy(
                (*from).inner.data.as_mut_ptr(),
                (*to).inner.data.as_mut_ptr(),
                BSIZE,
            );

            // Write the log.
            (*to).write();
            brelease(&mut *from);
            brelease(&mut *to);
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
    ///   bp = Buf::read(...)
    ///   modify bp->data[]
    ///   log_write(bp)
    ///   (*bp).release()
    pub unsafe fn log_write(&mut self, b: *mut Buf) {
        if self.lh.n >= LOGSIZE as i32 || self.lh.n >= self.size as i32 - 1 {
            panic!("too big a transaction");
        }
        if self.outstanding < 1 {
            panic!("log_write outside of trans");
        }
        self.lock.acquire();
        let mut absorbed = false;
        for i in 0..self.lh.n {
            // Log absorbtion.
            if self.lh.block[i as usize] as u32 == (*b).blockno {
                self.lh.block[i as usize] = (*b).blockno as i32;
                absorbed = true;
                break;
            }
        }

        // Add new block to log?
        if !absorbed {
            self.lh.block[self.lh.n as usize] = (*b).blockno as i32;
            bpin(&mut *b);
            self.lh.n += 1;
        }
        self.lock.release();
    }
}
