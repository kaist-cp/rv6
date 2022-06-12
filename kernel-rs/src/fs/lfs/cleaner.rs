use super::segment::DSegSum;
use super::{Lfs, Tx};
use crate::param::NBUF;
use crate::{hal::hal, param::SEGSIZE, proc::KernelCtx, util::strong_pin::StrongPin};

// TODO: Should we change the cleaner to bypass locks?
// However, we may want to allow multiple threads run the cleaner at the same time.

// TODO: We might be doing disk writes more than neccessary.
// We can just write the `Imap` or `Inode` to the disk only once.

const THRESHOLD: usize = 2;

impl Lfs {
    /// Scans the segment for live blocks.
    /// Returns a copy of the segment summary with dead blocks marked as empty, and the number of live blocks.
    /// Aborts the scan if the number of live blocks is larger than `thres`.
    fn scan_segment(
        &self,
        seg_no: u32,
        thres: usize,
        dev: u32,
        tx: &mut Tx<'_, Lfs>,
        ctx: &KernelCtx<'_, '_>,
    ) -> (DSegSum, usize) {
        // 1. read the segment summary block
        let superblock = self.superblock();
        let buf = hal()
            .disk()
            .read(dev, superblock.seg_to_disk_block_no(seg_no, 0), ctx);
        let mut seg_sum = unsafe { &*(buf.deref_inner().data.as_ptr() as *const DSegSum) }.clone();
        buf.free(ctx);

        // 2. iterate the segment summary and count the number of live blocks
        // or mark dead blocks as empty
        let mut live: usize = 0;
        let itable = unsafe { StrongPin::new_unchecked(self) }.itable();

        for i in 0..SEGSIZE - 1 {
            let entry = &mut seg_sum.0[i];
            let bno = superblock.seg_to_disk_block_no(seg_no, i as u32 + 1);

            // check whether the block is live
            let is_live = match entry.block_type {
                // empty
                0 => false,
                // inode
                1 => {
                    let imap = self.imap(ctx);
                    let res = bno == imap.get(entry.inum, ctx);
                    imap.free(ctx);
                    res
                }
                // data
                2 => {
                    // first, check whether the inode exists
                    let imap = self.imap(ctx);
                    let block_no = imap.get(entry.inum, ctx);
                    imap.free(ctx);
                    if block_no != 0 {
                        // now check whether the inode's `entry.block_no`th data block exists
                        // and is stored at `bno`
                        let inode = itable.get_inode(dev, entry.inum);
                        let ip = inode.lock(ctx);
                        let addr = ip.read_addr(entry.block_no as usize, ctx);
                        ip.free(ctx);
                        inode.free((tx, ctx));

                        if let Some(bno2) = addr {
                            bno == bno2
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                // indirect
                3 => {
                    // first, check whether the inode exists
                    let imap = self.imap(ctx);
                    let block_no = imap.get(entry.inum, ctx);
                    imap.free(ctx);
                    if block_no != 0 {
                        // now check whether the inode's indirect mapping block exists
                        // and is stored at `bno`
                        let inode = itable.get_inode(dev, entry.inum);
                        let ip = inode.lock(ctx);
                        let addr = ip.deref_inner().addr_indirect;
                        ip.free(ctx);
                        inode.free((tx, ctx));
                        bno == addr
                    } else {
                        false
                    }
                }
                // imap
                4 => {
                    let imap = self.imap(ctx);
                    let block_no = imap.get_nth_block(entry.block_no as usize);
                    imap.free(ctx);
                    bno == block_no
                }
                _ => panic!("cleaner : Not reach"),
            };

            if is_live {
                live += 1;
                if live > thres {
                    break;
                }
            } else {
                // dead block; mark as empty.
                entry.block_type = 0;
            }
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
        for i in 0..SEGSIZE - 1 {
            let entry = &seg_sum.0[i];
            match entry.block_type {
                0 => (), //empty
                1 => {
                    // inode
                    let inode = itable.get_inode(dev, entry.inum);
                    let ip = inode.lock(ctx);
                    ip.update(tx, ctx);
                    ip.free(ctx);
                    inode.free((tx, ctx))
                }
                2 => {
                    // data
                    let inode = itable.get_inode(dev, entry.inum);
                    let mut ip = inode.lock(ctx);

                    // copy to end of segment
                    let mut seg = self.segmanager(ctx);
                    ip.writable_data_block(entry.block_no as usize, &mut seg, tx, ctx)
                        .free(ctx);
                    if seg.is_full() {
                        seg.commit(ctx);
                    }
                    seg.free(ctx);

                    // update inode
                    ip.update(tx, ctx);
                    ip.free(ctx);
                    inode.free((tx, ctx));
                }
                3 => {
                    // indirect
                    let inode = itable.get_inode(dev, entry.inum);
                    let mut ip = inode.lock(ctx);

                    // copy to end of segment
                    let mut seg = self.segmanager(ctx);
                    ip.writable_indirect_block(&mut seg, ctx).free(ctx);
                    if seg.is_full() {
                        seg.commit(ctx);
                    }
                    seg.free(ctx);

                    // update inode
                    ip.update(tx, ctx);
                    ip.free(ctx);
                    inode.free((tx, ctx));
                }
                4 => {
                    //imap
                    let mut seg = self.segmanager(ctx);
                    let mut imap = self.imap(ctx);
                    imap.update(entry.block_no, &mut seg, ctx)
                        .unwrap()
                        .free(ctx);
                    if seg.is_full() {
                        seg.commit(ctx);
                    }
                    imap.free(ctx);
                    seg.free(ctx);
                }
                _ => panic!("cleaner : Not reach"),
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
            let seg_no = (last_seg_no + i + 1) % self.superblock().nsegments();
            // 1. check whether the `seg_no`th segment is marked as allocated.
            let seg = self.segmanager(ctx);
            let is_free = seg.segtable_is_free(seg_no);
            seg.free(ctx);
            if !is_free {
                // 2. scan the segment to count the number of live blocks
                let (seg_sum, live) = self.scan_segment(seg_no, THRESHOLD, dev, tx, ctx);
                if live <= THRESHOLD {
                    // 3. If the segment does not have a lot of live blocks,
                    // move its live blocks to another segment and mark it as free.
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
        }
        // TODO: We may need to panic in this case.
        last_seg_no
    }
}
