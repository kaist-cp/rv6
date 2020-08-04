use crate::libc;
use crate::{
    buf::Buf,
    param::NBUF,
    printf::panic,
    sleeplock::{acquiresleep, holdingsleep, initsleeplock, releasesleep},
    spinlock::{acquire, release, Spinlock},
    virtio_disk::virtio_disk_rw,
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
    bcache.lock.initlock(
        b"bcache\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    // Create linked list of buffers
    bcache.head.prev = &mut bcache.head;
    bcache.head.next = &mut bcache.head;
    b = bcache.buf.as_mut_ptr();
    while b < bcache.buf.as_mut_ptr().offset(NBUF as isize) {
        (*b).next = bcache.head.next;
        (*b).prev = &mut bcache.head;
        initsleeplock(
            &mut (*b).lock,
            b"buffer\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        );
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
    acquire(&mut bcache.lock);
    // Is the block already cached?
    b = bcache.head.next;
    while b != &mut bcache.head as *mut Buf {
        if (*b).dev == dev && (*b).blockno == blockno {
            (*b).refcnt = (*b).refcnt.wrapping_add(1);
            release(&mut bcache.lock);
            acquiresleep(&mut (*b).lock);
            return b;
        }
        b = (*b).next
    }

    // Not cached; recycle an unused buffer.
    b = bcache.head.prev;
    while b != &mut bcache.head as *mut Buf {
        if (*b).refcnt == 0 as i32 as u32 {
            (*b).dev = dev;
            (*b).blockno = blockno;
            (*b).valid = 0 as i32;
            (*b).refcnt = 1 as i32 as u32;
            release(&mut bcache.lock);
            acquiresleep(&mut (*b).lock);
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
    if (*b).valid == 0 {
        virtio_disk_rw(b, 0 as i32);
        (*b).valid = 1 as i32
    }
    b
}

/// Write b's contents to disk.  Must be locked.
pub unsafe fn bwrite(mut b: *mut Buf) {
    if holdingsleep(&mut (*b).lock) == 0 {
        panic(b"bwrite\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    virtio_disk_rw(b, 1 as i32);
}

/// Release a locked buffer.
/// Move to the head of the MRU list.
pub unsafe fn brelse(mut b: *mut Buf) {
    let bcache = BCACHE.get_mut();

    if holdingsleep(&mut (*b).lock) == 0 {
        panic(b"brelse\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    releasesleep(&mut (*b).lock);
    acquire(&mut bcache.lock);
    (*b).refcnt = (*b).refcnt.wrapping_sub(1);
    if (*b).refcnt == 0 as i32 as u32 {
        // no one is waiting for it.
        (*(*b).next).prev = (*b).prev;
        (*(*b).prev).next = (*b).next;
        (*b).next = bcache.head.next;
        (*b).prev = &mut bcache.head;
        (*bcache.head.next).prev = b;
        bcache.head.next = b
    }
    release(&mut bcache.lock);
}
pub unsafe fn bpin(mut b: *mut Buf) {
    let bcache = BCACHE.get_mut();

    acquire(&mut bcache.lock);
    (*b).refcnt = (*b).refcnt.wrapping_add(1);
    release(&mut bcache.lock);
}
pub unsafe fn bunpin(mut b: *mut Buf) {
    let bcache = BCACHE.get_mut();

    acquire(&mut bcache.lock);
    (*b).refcnt = (*b).refcnt.wrapping_sub(1);
    release(&mut bcache.lock);
}
