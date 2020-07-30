use crate::libc;
use crate::{
    buf::Buf,
    param::NBUF,
    printf::panic,
    proc::cpu,
    sleeplock::{acquiresleep, holdingsleep, initsleeplock, releasesleep, Sleeplock},
    spinlock::{acquire, initlock, release, Spinlock},
    virtio_disk::virtio_disk_rw,
};
use core::ptr;
/// Buffer cache.
///
/// The buffer cache is a linked list of buf structures holding
/// cached copies of disk block contents.  Caching disk blocks
/// in memory reduces the number of disk reads and also provides
/// a synchronization point for disk blocks used by multiple processes.
///
/// Interface:
/// * To get a buffer for a particular disk block, call bread.
/// * After changing buffer data, call bwrite to write it to disk.
/// * When done with the buffer, call brelse.
/// * Do not use the buffer after calling brelse.
/// * Only one process at a time can use a buffer,
///     so do not keep them longer than necessary.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Bcache {
    pub lock: Spinlock,
    pub buf: [Buf; 30],
    pub head: Buf,
}
#[no_mangle]
pub static mut bcache: Bcache = Bcache {
    lock: Spinlock {
        locked: 0,
        name: 0 as *const libc::c_char as *mut libc::c_char,
        cpu: 0 as *const cpu as *mut cpu,
    },
    buf: [Buf {
        valid: 0,
        disk: 0,
        dev: 0,
        blockno: 0,
        lock: Sleeplock {
            locked: 0,
            lk: Spinlock {
                locked: 0,
                name: 0 as *const libc::c_char as *mut libc::c_char,
                cpu: 0 as *const cpu as *mut cpu,
            },
            name: 0 as *const libc::c_char as *mut libc::c_char,
            pid: 0,
        },
        refcnt: 0,
        prev: 0 as *const Buf as *mut Buf,
        next: 0 as *const Buf as *mut Buf,
        qnext: 0 as *const Buf as *mut Buf,
        data: [0; 1024],
    }; 30],
    head: Buf {
        valid: 0,
        disk: 0,
        dev: 0,
        blockno: 0,
        lock: Sleeplock {
            locked: 0,
            lk: Spinlock {
                locked: 0,
                name: 0 as *const libc::c_char as *mut libc::c_char,
                cpu: 0 as *const cpu as *mut cpu,
            },
            name: 0 as *const libc::c_char as *mut libc::c_char,
            pid: 0,
        },
        refcnt: 0,
        prev: 0 as *const Buf as *mut Buf,
        next: 0 as *const Buf as *mut Buf,
        qnext: 0 as *const Buf as *mut Buf,
        data: [0; 1024],
    },
};
#[no_mangle]
pub unsafe extern "C" fn binit() {
    let mut b: *mut Buf = ptr::null_mut();
    initlock(
        &mut bcache.lock,
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
unsafe extern "C" fn bget(mut dev: u32, mut blockno: u32) -> *mut Buf {
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
#[no_mangle]
pub unsafe extern "C" fn bread(mut dev: u32, mut blockno: u32) -> *mut Buf {
    let mut b: *mut Buf = ptr::null_mut();
    b = bget(dev, blockno);
    if (*b).valid == 0 {
        virtio_disk_rw(b, 0 as i32);
        (*b).valid = 1 as i32
    }
    b
}
/// Write b's contents to disk.  Must be locked.
#[no_mangle]
pub unsafe extern "C" fn bwrite(mut b: *mut Buf) {
    if holdingsleep(&mut (*b).lock) == 0 {
        panic(b"bwrite\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    virtio_disk_rw(b, 1 as i32);
}
/// Release a locked buffer.
/// Move to the head of the MRU list.
#[no_mangle]
pub unsafe extern "C" fn brelse(mut b: *mut Buf) {
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
#[no_mangle]
pub unsafe extern "C" fn bpin(mut b: *mut Buf) {
    acquire(&mut bcache.lock);
    (*b).refcnt = (*b).refcnt.wrapping_add(1);
    release(&mut bcache.lock);
}
#[no_mangle]
pub unsafe extern "C" fn bunpin(mut b: *mut Buf) {
    acquire(&mut bcache.lock);
    (*b).refcnt = (*b).refcnt.wrapping_sub(1);
    release(&mut bcache.lock);
}
