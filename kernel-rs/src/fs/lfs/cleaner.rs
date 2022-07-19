use super::{
    segment::{BlockType, DSegSum, DSegSumEntry, SEGSUM_MAGIC},
    Lfs, Tx,
};
use crate::{
    hal::hal,
    param::{NBUF, SEGSIZE},
    proc::KernelCtx,
    util::strong_pin::StrongPin,
};

// TODO: We might be doing disk writes more than neccessary.
// We can just write the `Imap` or `Inode` to the disk only once.

const THRESHOLD: usize = 2;

impl Lfs {
    /// Checks whether the block stored at `bno` is live or not.
    /// The given `entry` must be the segment summary entry of the block stored at `bno`.
    /// Returns true if the block is live. Otherwise, returns false.
    fn scan_block(
        &self,
        dev: u32,
        bno: u32,
        entry: &DSegSumEntry,
        tx: &Tx<'_, Lfs>,
        ctx: &KernelCtx<'_, '_>,
    ) -> bool {
        // Check t < #(variants of BlockType) to prevent UB.
        let t = unsafe { *(entry as *const DSegSumEntry as *const u32) };
        assert!(t < core::mem::variant_count::<BlockType>() as u32);

        let itable = unsafe { StrongPin::new_unchecked(self) }.itable();

        // check whether the block is live
        match entry.block_type {
            BlockType::Empty => false,
            BlockType::Inode => {
                let imap = self.imap(ctx);
                let res = bno == imap.get(entry.inum, ctx);
                imap.free(ctx);
                res
            }
            BlockType::DataBlock => {
                // first, check whether the inode exists
                let imap = self.imap(ctx);
                let block_no = imap.get(entry.inum, ctx);
                imap.free(ctx);
                if block_no == 0 {
                    return false;
                }
                // now check whether the inode's `entry.block_no`th data block exists
                // and is stored at `bno`
                let inode = itable.get_inode(dev, entry.inum);
                let ip = inode.lock(ctx);
                let addr = ip.read_addr(entry.block_no as usize, ctx);
                ip.free(ctx);
                inode.free((tx, ctx));

                if addr.is_none() {
                    return false;
                }
                bno == addr.unwrap()
            }
            BlockType::IndirectMap => {
                // first, check whether the inode exists
                let imap = self.imap(ctx);
                let block_no = imap.get(entry.inum, ctx);
                imap.free(ctx);
                if block_no == 0 {
                    return false;
                }
                // now check whether the inode's indirect mapping block exists
                // and is stored at `bno`
                let inode = itable.get_inode(dev, entry.inum);
                let ip = inode.lock(ctx);
                let addr = ip.deref_inner().addr_indirect;
                ip.free(ctx);
                inode.free((tx, ctx));
                bno == addr
            }
            BlockType::Imap => {
                let imap = self.imap(ctx);
                let block_no = imap.get_nth_block(entry.block_no as usize);
                imap.free(ctx);
                bno == block_no
            }
        }
    }

    /// Scans the entries of the segment summary block located at `seg_block_no` for live blocks.
    /// Returns `None` if the block located at `seg_block_no` is not a segment summary block.
    /// Otherwise, returns a copy of the segment summary, where dead blocks marked as empty, and the number of live blocks.
    /// Aborts the scan if the number of live blocks is larger than `thres`.
    fn scan_seg_sum(
        &self,
        seg_no: u32,
        seg_block_no: u32,
        thres: usize,
        dev: u32,
        tx: &mut Tx<'_, Lfs>,
        ctx: &KernelCtx<'_, '_>,
    ) -> Option<(DSegSum, usize)> {
        // 1. read the segment summary block
        let superblock = self.superblock();
        let buf = hal().disk().read(
            dev,
            superblock.seg_to_disk_block_no(seg_no, seg_block_no),
            ctx,
        );
        let mut seg_sum = unsafe { &*(buf.deref_inner().data.as_ptr() as *const DSegSum) }.clone();
        buf.free(ctx);
        if seg_sum.magic != SEGSUM_MAGIC {
            return None;
        }

        // 2. iterate the segment summary and count the number of live blocks
        // or mark dead blocks as empty
        let mut live: usize = 0;
        for i in 0..seg_sum.size as usize {
            let bno = superblock.seg_to_disk_block_no(seg_no, seg_block_no + 1 + i as u32);
            if self.scan_block(dev, bno, &seg_sum.entries[i], tx, ctx) {
                live += 1;
                if live > thres {
                    break;
                }
            } else {
                // dead block; mark as empty.
                seg_sum.entries[i].block_type = BlockType::Empty;
            }
        }
        Some((seg_sum, live))
    }

    /// Scans the segment for live blocks.
    /// Returns a segment summary, where everything except live blocks marked as empty, and the number of live blocks.
    /// Aborts the scan if the number of live blocks is larger than `thres`.
    fn scan_segment(
        &self,
        seg_no: u32,
        thres: usize,
        dev: u32,
        tx: &mut Tx<'_, Lfs>,
        ctx: &KernelCtx<'_, '_>,
    ) -> (DSegSum, usize) {
        let mut seg_sum = DSegSum::default();
        let mut curr: usize = 0;
        let mut live = 0;

        loop {
            match self.scan_seg_sum(seg_no, curr as u32, thres - live, dev, tx, ctx) {
                None => break,
                Some((curr_seg_sum, curr_live)) => {
                    live += curr_live;
                    if live > thres {
                        break;
                    }

                    assert!((curr + curr_seg_sum.size as usize) < SEGSIZE);
                    // Mark the entry for the segment summary block as `Empty`.
                    if curr > 0 {
                        seg_sum.entries[curr - 1] = DSegSumEntry::default();
                    }
                    // Copy entries of the `curr_seg_sum` to `seg_sum`.
                    for i in 0..curr_seg_sum.size as usize {
                        seg_sum.entries[curr + i] = curr_seg_sum.entries[i];
                    }

                    curr += curr_seg_sum.size as usize + 1;
                    if curr >= SEGSIZE - 1 {
                        // there can't be any more blocks
                        break;
                    }
                }
            }
        }
        if curr > 0 {
            seg_sum.size = curr as u32 - 1;
        }

        (seg_sum, live)
    }

    /// Moves all blocks that are not marked as dead in the given `seg_sum` to another segment,
    /// and mark the current segment as free on the segtable.
    fn clean_segment(
        &self,
        seg_sum: &DSegSum,
        seg_no: u32,
        dev: u32,
        tx: &mut Tx<'_, Lfs>,
        ctx: &KernelCtx<'_, '_>,
    ) {
        let itable = unsafe { StrongPin::new_unchecked(self) }.itable();
        // iterate the segment summary and move live blocks to the current segment.
        for i in 0..seg_sum.size as usize {
            let entry = &seg_sum.entries[i];
            match entry.block_type {
                BlockType::Empty => (),
                BlockType::Inode => {
                    let inode = itable.get_inode(dev, entry.inum);
                    let ip = inode.lock(ctx);
                    ip.update(tx, ctx);
                    ip.free(ctx);
                    inode.free((tx, ctx))
                }
                BlockType::DataBlock => {
                    let inode = itable.get_inode(dev, entry.inum);
                    let mut ip = inode.lock(ctx);

                    // copy to end of segment
                    let mut seg = self.segmanager(ctx);
                    ip.writable_data_block(entry.block_no as usize, &mut seg, tx, ctx)
                        .free(ctx);
                    if seg.is_full() {
                        seg.commit(true, ctx);
                    }
                    seg.free(ctx);

                    // update inode
                    ip.update(tx, ctx);
                    ip.free(ctx);
                    inode.free((tx, ctx));
                }
                BlockType::IndirectMap => {
                    let inode = itable.get_inode(dev, entry.inum);
                    let mut ip = inode.lock(ctx);

                    // copy to end of segment
                    let mut seg = self.segmanager(ctx);
                    ip.writable_indirect_block(&mut seg, ctx).free(ctx);
                    if seg.is_full() {
                        seg.commit(true, ctx);
                    }
                    seg.free(ctx);

                    // update inode
                    ip.update(tx, ctx);
                    ip.free(ctx);
                    inode.free((tx, ctx));
                }
                BlockType::Imap => {
                    let mut seg = self.segmanager(ctx);
                    let mut imap = self.imap(ctx);
                    imap.update(entry.block_no, &mut seg, ctx)
                        .unwrap()
                        .free(ctx);
                    if seg.is_full() {
                        seg.commit(true, ctx);
                    }
                    imap.free(ctx);
                    seg.free(ctx);
                }
            };
        }

        // mark segment as free
        let mut seg = self.segmanager(ctx);
        seg.segtable_free(seg_no);
        seg.free(ctx);
    }

    pub fn clean(
        &self,
        last_seg_no: u32,
        dev: u32,
        tx: &mut Tx<'_, Lfs>,
        ctx: &KernelCtx<'_, '_>,
    ) -> u32 {
        for i in 0..self.superblock().nsegments() {
            // 1. check whether the segment is marked as allocated.
            let seg_no = (last_seg_no + i + 1) % self.superblock().nsegments();
            let seg = self.segmanager(ctx);
            let is_free = seg.segtable_is_free(seg_no);
            seg.free(ctx);
            if is_free {
                continue;
            }

            // 2. scan the segment to count the number of live blocks
            let (seg_sum, live) = self.scan_segment(seg_no, THRESHOLD, dev, tx, ctx);

            // 3. if the segment does not have a lot of live blocks,
            // move its live blocks to another segment and mark it as free.
            if live <= THRESHOLD {
                self.clean_segment(&seg_sum, seg_no, dev, tx, ctx);
            }

            // 4. stop if we now have enough segments
            let seg = self.segmanager(ctx);
            let nfree = seg.nfree();
            seg.free(ctx);
            if nfree * (SEGSIZE as u32) > 4 * NBUF as u32 {
                return seg_no;
            }
        }
        // TODO: We may need to panic in this case.
        last_seg_no
    }
}
