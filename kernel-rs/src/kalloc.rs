//! Physical memory allocator, for user processes,
//! kernel stacks, page-table pages,
//! and pipe buffers. Allocates whole 4096-byte pages.
use crate::libc;
use crate::{
    memlayout::PHYSTOP,
    printf::panic,
    riscv::{pgroundup, PGSIZE},
    spinlock::Spinlock,
};
use core::ptr;

/// first address after kernel.
/// defined by kernel.ld.
pub static mut END: [u8; 0] = [0; 0];

#[derive(Copy, Clone)]
struct Run {
    next: *mut Run,
}

struct Kmem {
    lock: Spinlock,
    freelist: *mut Run,
}

impl Kmem {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            lock: Spinlock::zeroed(),
            freelist: ptr::null_mut(),
        }
    }
}

static mut KMEM: Kmem = Kmem::zeroed();

pub unsafe fn kinit() {
    KMEM.lock
        .initlock(b"KMEM\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);

    // TODO: without this strange code, the kernel doesn't boot up.  Probably stack is not properly
    // initialized at the beginning...
    let protection = 0;
    drop(protection);

    freerange(
        END.as_mut_ptr() as *mut libc::CVoid,
        PHYSTOP as *mut libc::CVoid,
    );
}

pub unsafe fn freerange(pa_start: *mut libc::CVoid, pa_end: *mut libc::CVoid) {
    let mut p = pgroundup(pa_start as usize) as *mut libc::CChar;
    while p.offset(PGSIZE as isize) <= pa_end as *mut libc::CChar {
        kfree(p as *mut libc::CVoid);
        p = p.offset(PGSIZE as isize)
    }
}

/// Free the page of physical memory pointed at by v,
/// which normally should have been returned by a
/// call to kalloc().  (The exception is when
/// initializing the allocator; see kinit above.)
pub unsafe fn kfree(pa: *mut libc::CVoid) {
    if (pa as usize).wrapping_rem(PGSIZE as usize) != 0
        || (pa as *mut libc::CChar) < END.as_mut_ptr()
        || pa as usize >= PHYSTOP as usize
    {
        panic(b"kfree\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
    }

    // Fill with junk to catch dangling refs.
    ptr::write_bytes(pa as *mut libc::CVoid, 1, PGSIZE as usize);
    let mut r: *mut Run = pa as *mut Run;
    KMEM.lock.acquire();
    (*r).next = KMEM.freelist;
    KMEM.freelist = r;
    KMEM.lock.release();
}

/// Allocate one 4096-byte page of physical memory.
/// Returns a pointer that the kernel can use.
/// Returns 0 if the memory cannot be allocated.
pub unsafe fn kalloc() -> *mut libc::CVoid {
    KMEM.lock.acquire();
    let r: *mut Run = KMEM.freelist;
    if !r.is_null() {
        KMEM.freelist = (*r).next
    }
    KMEM.lock.release();
    if !r.is_null() {
        // fill with junk
        ptr::write_bytes(
            r as *mut libc::CChar as *mut libc::CVoid,
            5,
            PGSIZE as usize,
        );
    }
    r as *mut libc::CVoid
}
