//! Physical memory allocator, for user processes,
//! kernel stacks, page-table pages,
//! and pipe buffers. Allocates whole 4096-byte pages.
use crate::libc;
use crate::{
    memlayout::PHYSTOP,
    printf::panic,
    riscv::{pgroundup, PGSIZE},
    spinlock::RawSpinlock,
};
use core::ptr;

extern "C" {
    // first address after kernel.
    // defined by kernel.ld.
    #[no_mangle]
    static mut end: [u8; 0];
}

#[derive(Copy, Clone)]
struct Run {
    next: *mut Run,
}

struct Kmem {
    lock: RawSpinlock,
    freelist: *mut Run,
}

impl Kmem {
    // TODO: transient measure
    pub const fn zeroed() -> Self {
        Self {
            lock: RawSpinlock::zeroed(),
            freelist: ptr::null_mut(),
        }
    }
}

static mut KMEM: Kmem = Kmem::zeroed();

pub unsafe fn kinit() {
    KMEM.lock.initlock(b"KMEM\x00" as *const u8 as *mut u8);

    // TODO: without this strange code, the kernel doesn't boot up.  Probably stack is not properly
    // initialized at the beginning...
    let protection = 0;
    drop(protection);

    freerange(
        end.as_mut_ptr() as *mut libc::CVoid,
        PHYSTOP as *mut libc::CVoid,
    );
}

pub unsafe fn freerange(pa_start: *mut libc::CVoid, pa_end: *mut libc::CVoid) {
    let mut p = pgroundup(pa_start as usize) as *mut u8;
    while p.add(PGSIZE) <= pa_end as *mut u8 {
        kfree(p as *mut libc::CVoid);
        p = p.add(PGSIZE)
    }
}

/// Free the page of physical memory pointed at by v,
/// which normally should have been returned by a
/// call to kalloc().  (The exception is when
/// initializing the allocator; see kinit above.)
pub unsafe fn kfree(pa: *mut libc::CVoid) {
    if (pa as usize).wrapping_rem(PGSIZE) != 0
        || (pa as *mut u8) < end.as_mut_ptr()
        || pa as usize >= PHYSTOP
    {
        panic(b"kfree\x00" as *const u8 as *mut u8);
    }

    // Fill with junk to catch dangling refs.
    ptr::write_bytes(pa as *mut libc::CVoid, 1, PGSIZE);
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
        ptr::write_bytes(r as *mut u8 as *mut libc::CVoid, 5, PGSIZE);
    }
    r as *mut libc::CVoid
}
