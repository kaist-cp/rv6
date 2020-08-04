use crate::libc;
use crate::{
    buf::Buf, param::NBUF, printf::panic, spinlock::Spinlock, virtio_disk::virtio_disk_rw,
};
use core::mem::MaybeUninit;
use core::ptr;

/// Buffer cache.
///
/// The buffer cache is a linked list of buf structures holding cached copies of disk block
/// contents.  Caching disk blocks in memory reduces the number of disk reads and also provides a
/// synchronization point for disk blocks used by multiple processes.
///
/// Interface:
/// * To get a buffer for a particular disk block, call bread.
/// * After changing buffer data, call bwrite to write it to disk.
/// * When done with the buffer, call brelse.
/// * Do not use the buffer after calling brelse.
/// * Only one process at a time can use a buffer, so do not keep them longer than necessary.
pub struct Bcache {
    lock: Spinlock,
    buf: [Buf; NBUF as usize],

    // Linked list of all buffers, through prev/next.  head.next is most recently used.
    head: Buf,
}

pub static mut BCACHE: MaybeUninit<Bcache> = MaybeUninit::uninit();

pub unsafe fn binit() {
    let bcache = BCACHE.get_mut();

    let mut b: *mut Buf = ptr::null_mut();
    bcache
        .lock
        .initlock(b"bcache\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    // Create linked list of buffers
    bcache.head.prev = &mut bcache.head;
    bcache.head.next = &mut bcache.head;
    b = bcache.buf.as_mut_ptr();
    while b < bcache.buf.as_mut_ptr().offset(NBUF as isize) {
        (*b).next = bcache.head.next;
        (*b).prev = &mut bcache.head;
        (*b).lock
            .initlock(b"buffer\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        (*bcache.head.next).prev = b;
        bcache.head.next = b;
        b = b.offset(1)
    }
}

/// Look through buffer cache for block on device dev.
/// If not found, allocate a buffer.
/// In either case, return locked buffer.
unsafe fn bget(mut dev: u32, mut blockno: u32) -> *mut Buf {
    let bcache = BCACHE.get_mut();

    let mut b: *mut Buf = ptr::null_mut();
    bcache.lock.acquire();
    // Is the block already cached?
    b = bcache.head.next;
    while b != &mut bcache.head as *mut Buf {
        if (*b).getdev() == dev && (*b).getblockno() == blockno {
            (*b).increfcnt();
            bcache.lock.release();
            (*b).lock.acquire();
            return b;
        }
        b = (*b).next
    }

    // Not cached; recycle an unused buffer.
    b = bcache.head.prev;
    while b != &mut bcache.head as *mut Buf {
        if (*b).getrefcnt() == 0 as i32 as u32 {
            (*b).setdev(dev);
            (*b).setblockno(blockno);
            (*b).setvalid(0);
            (*b).setrefcnt(1);
            bcache.lock.release();
            (*b).lock.acquire();
            return b;
        }
        b = (*b).prev
    }
    panic(b"bget: no buffers\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
}

/// Return a locked buf with the contents of the indicated block.
pub unsafe fn bread(mut dev: u32, mut blockno: u32) -> *mut Buf {
    let mut b: *mut Buf = ptr::null_mut();
    b = bget(dev, blockno);
    if (*b).getvalid() == 0 {
        virtio_disk_rw(b, 0 as i32);
        (*b).setvalid(1);
    }
    b
}

impl Buf {
    /// Write b's contents to disk.  Must be locked.
    pub unsafe fn bwrite(&mut self) {
        if (*self).lock.holding() == 0 {
            panic(b"bwrite\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        virtio_disk_rw(self, 1 as i32);
    }

    /// Release a locked buffer.
    /// Move to the head of the MRU list.
    pub unsafe fn brelse(&mut self) {
        let bcache = BCACHE.get_mut();

        if (*self).lock.holding() == 0 {
            panic(b"brelse\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        (*self).lock.release();
        bcache.lock.acquire();
        let refcnt = (*self).getrefcnt();
        (*self).setrefcnt(refcnt.wrapping_sub(1));
        if (*self).getrefcnt() == 0 as i32 as u32 {
            // no one is waiting for it.
            (*(*self).next).prev = (*self).prev;
            (*(*self).prev).next = (*self).next;
            (*self).next = bcache.head.next;
            (*self).prev = &mut bcache.head;
            (*bcache.head.next).prev = self;
            bcache.head.next = self
        }
        bcache.lock.release();
    }

    pub unsafe fn bpin(&mut self) {
        let bcache = BCACHE.get_mut();

        bcache.lock.acquire();
        let refcnt = (*self).getrefcnt();
        (*self).setrefcnt(refcnt.wrapping_add(1));
        bcache.lock.release();
    }

    pub unsafe fn bunpin(&mut self) {
        let bcache = BCACHE.get_mut();

        bcache.lock.acquire();
        let refcnt = (*self).getrefcnt();
        (*self).setrefcnt(refcnt.wrapping_sub(1));
        bcache.lock.release();
    }
}
