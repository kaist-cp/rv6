//! Physical memory allocator, for user processes,
//! kernel stacks, page-table pages,
//! and pipe buffers. Allocates whole 4096-byte pages.
use crate::libc;
use crate::{memlayout::PHYSTOP, printf::panic, riscv::PGSIZE, spinlock::Spinlock};
use core::ptr;

/// first address after kernel.
/// defined by kernel.ld.
pub static mut end: [u8; 0] = [0; 0];

#[derive(Copy, Clone)]
struct Run {
    next: *mut Run,
}

#[derive(Copy, Clone)]
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

static mut kmem: Kmem = Kmem::zeroed();

pub unsafe fn kinit() {
    kmem.lock
        .initlock(b"kmem\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);

    freerange(
        end.as_mut_ptr() as *mut libc::c_void,
        PHYSTOP as *mut libc::c_void,
    );
}

pub unsafe fn freerange(mut pa_start: *mut libc::c_void, mut pa_end: *mut libc::c_void) {
    let mut p = ((pa_start as usize)
        .wrapping_add(PGSIZE as usize)
        .wrapping_sub(1)
        & !(PGSIZE - 1) as usize) as *mut libc::c_char;
    while p.offset(PGSIZE as isize) <= pa_end as *mut libc::c_char {
        kfree(p as *mut libc::c_void);
        p = p.offset(PGSIZE as isize)
    }
}

/// Free the page of physical memory pointed at by v,
/// which normally should have been returned by a
/// call to kalloc().  (The exception is when
/// initializing the allocator; see kinit above.)
pub unsafe fn kfree(mut pa: *mut libc::c_void) {
    let mut r: *mut Run = ptr::null_mut();
    if (pa as usize).wrapping_rem(PGSIZE as usize) != 0
        || (pa as *mut libc::c_char) < end.as_mut_ptr()
        || pa as usize >= PHYSTOP as usize
    {
        panic(b"kfree\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }

    // Fill with junk to catch dangling refs.
    ptr::write_bytes(pa as *mut libc::c_void, 1, PGSIZE as usize);
    r = pa as *mut Run;
    kmem.lock.acquire();
    (*r).next = kmem.freelist;
    kmem.freelist = r;
    kmem.lock.release();
}

/// Allocate one 4096-byte page of physical memory.
/// Returns a pointer that the kernel can use.
/// Returns 0 if the memory cannot be allocated.
pub unsafe fn kalloc() -> *mut libc::c_void {
    let mut r: *mut Run = ptr::null_mut();
    kmem.lock.acquire();
    r = kmem.freelist;
    if !r.is_null() {
        kmem.freelist = (*r).next
    }
    kmem.lock.release();
    if !r.is_null() {
        // fill with junk
        ptr::write_bytes(
            r as *mut libc::c_char as *mut libc::c_void,
            5,
            PGSIZE as usize,
        );
    }
    r as *mut libc::c_void
}
