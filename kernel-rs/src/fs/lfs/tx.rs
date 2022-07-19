//! The `TxManager` type manages the starts and wrap ups of FS transactions
//! to maintain consistency and an enough number of segments.
//!
//! * Blocks new FS sys calls when we may not have enough segments. (i.e. Wait for the segment cleaner to finish.)
//! * After all FS sys calls are done, commits the checkpoint.
//! * After all FS sys calls are done, runs the segment cleaner if the number of remaining segments
//!   are lower than threshold.

use super::{Lfs, Tx};
use crate::{
    lock::SleepableLock,
    param::{MAXOPBLOCKS, NBUF, SEGSIZE},
    proc::KernelCtx,
};

/// Manages the starts and wrap ups of FS transactions.
/// * Blocks new FS sys calls when we may not have enough segments. (i.e. Wait for the segment cleaner to finish.)
/// * After all FS sys calls are done, commits the checkpoint.
/// * After all FS sys calls are done, runs the segment cleaner if the number of remaining segments
///   are lower than threshold.
pub struct TxManager {
    dev: u32,

    /// How many FS sys calls are executing?
    outstanding: i32,

    /// In commit(), please wait.
    committing: bool,

    /// Stores whether the latest checkpoint is stored at the first checkpoint region or the second.
    stored_at_first: bool,

    /// The timestamp of the latest checkpoint.
    /// Increments when commiting the checkpoint.
    timestamp: u32,

    /// The last segment that the cleaner scanned.
    last_seg_no: u32,
}

impl TxManager {
    /// Returns a new `TxManager`.
    /// * `stored_at_first` should be `true` if the latest checkpoint is stored at the first checkpoint region.
    ///   Otherwise, it should be `false`.
    /// * `timestamp` should be the timestamp of the latest checkpoint.
    pub fn new(dev: u32, stored_at_first: bool, timestamp: u32) -> Self {
        Self {
            dev,
            outstanding: 0,
            committing: false,
            stored_at_first,
            timestamp,
            last_seg_no: 0,
        }
    }
}

impl SleepableLock<TxManager> {
    /// Called at the start of each FS system call.
    pub fn begin_op(&self, fs: &Lfs, ctx: &KernelCtx<'_, '_>) {
        let mut seg = fs.segmanager(ctx);
        let mut guard = self.lock();
        loop {
            let nfree = seg.nfree();
            seg.free(ctx);

            if guard.committing ||
            // This op might exhaust segments; wait for cleaner.
            nfree * (SEGSIZE as u32) < (guard.outstanding as u32 + 1) * MAXOPBLOCKS as u32 + NBUF as u32
            {
                guard.sleep(ctx);
                // TODO: Use a better way. (Add a lock and a waitchannel inside `TxManager` instead?)
                seg = guard.reacquire_after(|| fs.segmanager(ctx));
            } else {
                guard.outstanding += 1;
                break;
            }
        }
    }

    /// Called at the end of each FS system call.
    /// Commits the checkpoint if this was the last outstanding operation.
    pub fn end_op(&self, fs: &Lfs, tx: &mut Tx<'_, Lfs>, ctx: &KernelCtx<'_, '_>) {
        let mut guard = self.lock();
        guard.outstanding -= 1;
        assert!(!guard.committing, "guard.committing");

        if guard.outstanding == 0 {
            // Since outstanding is 0, no ongoing transaction exists.
            // The lock is still held, so new transactions cannot start.
            guard.committing = true;
            // Committing is true, so new transactions cannot start even after releasing the lock.

            // Update info about the latest checkpoint.
            guard.stored_at_first = !guard.stored_at_first;
            guard.timestamp += 1;

            // Store info before releasing the lock.
            let dev = guard.dev;
            let stored_at_first = guard.stored_at_first;
            let timestamp = guard.timestamp;
            let mut last_seg_no = guard.last_seg_no;

            guard.reacquire_after(|| {
                let seg = fs.segmanager(ctx);
                let nfree = seg.nfree();
                seg.free(ctx);
                if nfree * (SEGSIZE as u32) < 2 * NBUF as u32 {
                    last_seg_no = fs.clean(last_seg_no, dev, tx, ctx);
                }

                let mut seg = fs.segmanager(ctx);
                seg.commit(false, ctx);
                seg.free(ctx);
                // SAFETY: there is no another transaction, so `inner` cannot be read or written.
                unsafe {
                    // TODO: Checkpointing doesn't need to be done this often.
                    fs.commit_checkpoint(dev, stored_at_first, timestamp, ctx)
                }
            });

            guard.last_seg_no = last_seg_no;
            guard.committing = false;
        }

        // begin_op() may be waiting for LOG space, and decrementing log.outstanding has decreased
        // the amount of reserved space.
        guard.wakeup(ctx.kernel());
    }
}
