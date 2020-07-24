use crate::libc;
use core::ptr;
use crate::spinlock::{ Spinlock, acquire, initlock, release };
use crate::sleeplock::{ Sleeplock, acquiresleep, releasesleep, holdingsleep, initsleeplock };
use crate::proc::cpu;
use crate::buf::Buf;
extern "C" {
    // pub type cpu;
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    // // spinlock.c
    // #[no_mangle]
    // fn acquire(_: *mut spinlock);
    // #[no_mangle]
    // fn initlock(_: *mut spinlock, _: *mut libc::c_char);
    // #[no_mangle]
    // fn release(_: *mut spinlock);
    // sleeplock.c
    // #[no_mangle]
    // fn acquiresleep(_: *mut sleeplock);
    // #[no_mangle]
    // fn releasesleep(_: *mut sleeplock);
    // #[no_mangle]
    // fn holdingsleep(_: *mut sleeplock) -> libc::c_int;
    // #[no_mangle]
    // fn initsleeplock(_: *mut sleeplock, _: *mut libc::c_char);
    #[no_mangle]
    fn virtio_disk_rw(_: *mut Buf, _: libc::c_int);
}
pub type uint = libc::c_uint;
pub type uchar = libc::c_uchar;
// Mutual exclusion lock.
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct spinlock {
//     pub locked: uint,
//     pub name: *mut libc::c_char,
//     pub cpu: *mut cpu,
// }
// Long-term locks for processes
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct sleeplock {
//     pub locked: uint,
//     pub lk: spinlock,
//     pub name: *mut libc::c_char,
//     pub pid: libc::c_int,
// }
// #[derive(Copy, Clone)]
// #[repr(C)]
// pub struct buf {
//     pub valid: libc::c_int,
//     pub disk: libc::c_int,
//     pub dev: uint,
//     pub blockno: uint,
//     pub lock: sleeplock,
//     pub refcnt: uint,
//     pub prev: *mut buf,
//     pub next: *mut buf,
//     pub qnext: *mut buf,
//     pub data: [uchar; 1024],
// }
// Buffer cache.
//
// The buffer cache is a linked list of buf structures holding
// cached copies of disk block contents.  Caching disk blocks
// in memory reduces the number of disk reads and also provides
// a synchronization point for disk blocks used by multiple processes.
//
// Interface:
// * To get a buffer for a particular disk block, call bread.
// * After changing buffer data, call bwrite to write it to disk.
// * When done with the buffer, call brelse.
// * Do not use the buffer after calling brelse.
// * Only one process at a time can use a buffer,
//     so do not keep them longer than necessary.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct C2RustUnnamed {
    pub lock: Spinlock,
    pub buf: [Buf; 30],
    pub head: Buf,
}
// maximum number of processes
// maximum number of CPUs
// open files per process
// open files per system
// maximum number of active i-nodes
// maximum major device number
// device number of file system root disk
// max exec arguments
pub const MAXOPBLOCKS: libc::c_int = 10 as libc::c_int;
// max # of blocks any FS op writes
// max data blocks in on-disk log
pub const NBUF: libc::c_int = MAXOPBLOCKS * 3 as libc::c_int;
#[no_mangle]
pub static mut bcache: C2RustUnnamed = C2RustUnnamed {
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
// bio.c
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
// Look through buffer cache for block on device dev.
// If not found, allocate a buffer.
// In either case, return locked buffer.
unsafe extern "C" fn bget(mut dev: uint, mut blockno: uint) -> *mut Buf {
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
        if (*b).refcnt == 0 as libc::c_int as libc::c_uint {
            (*b).dev = dev;
            (*b).blockno = blockno;
            (*b).valid = 0 as libc::c_int;
            (*b).refcnt = 1 as libc::c_int as uint;
            release(&mut bcache.lock);
            acquiresleep(&mut (*b).lock);
            return b;
        }
        b = (*b).prev
    }
    panic(b"bget: no buffers\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
}
// Return a locked buf with the contents of the indicated block.
#[no_mangle]
pub unsafe extern "C" fn bread(mut dev: uint, mut blockno: uint) -> *mut Buf {
    let mut b: *mut Buf = ptr::null_mut();
    b = bget(dev, blockno);
    if (*b).valid == 0 {
        virtio_disk_rw(b, 0 as libc::c_int);
        (*b).valid = 1 as libc::c_int
    }
    b
}
// Write b's contents to disk.  Must be locked.
#[no_mangle]
pub unsafe extern "C" fn bwrite(mut b: *mut Buf) {
    if holdingsleep(&mut (*b).lock) == 0 {
        panic(b"bwrite\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    virtio_disk_rw(b, 1 as libc::c_int);
}
// Release a locked buffer.
// Move to the head of the MRU list.
#[no_mangle]
pub unsafe extern "C" fn brelse(mut b: *mut Buf) {
    if holdingsleep(&mut (*b).lock) == 0 {
        panic(b"brelse\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    releasesleep(&mut (*b).lock);
    acquire(&mut bcache.lock);
    (*b).refcnt = (*b).refcnt.wrapping_sub(1);
    if (*b).refcnt == 0 as libc::c_int as libc::c_uint {
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
