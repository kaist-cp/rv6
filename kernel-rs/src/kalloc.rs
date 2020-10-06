//! Physical memory allocator, for user processes,
//! kernel stacks, page-table pages,
//! and pipe buffers. Allocates whole 4096-byte pages.
use crate::{
    memlayout::PHYSTOP,
    riscv::{pgroundup, PGSIZE},
    spinlock::Spinlock,
};

use core::mem;
use core::ptr;

extern "C" {
    // first address after kernel.
    // defined by kernel.ld.
    #[no_mangle]
    static mut end: [u8; 0];
}

struct Run {
    next: *mut Run,
}

static mut KMEM: Spinlock<*mut Run> = Spinlock::new("KMEM", ptr::null_mut());

pub unsafe fn kinit() {
    freerange(end.as_mut_ptr(), PHYSTOP as _);
}

pub unsafe fn freerange(pa_start: *mut u8, pa_end: *mut u8) {
    let mut p = pgroundup(pa_start as _) as *mut u8;
    while p.add(PGSIZE) <= pa_end {
        kfree(p);
        p = p.add(PGSIZE)
    }
}

/// Free the page of physical memory pointed at by v,
/// which normally should have been returned by a
/// call to kalloc().  (The exception is when
/// initializing the allocator; see kinit above.)
pub unsafe fn kfree(pa: *mut u8) {
    if (pa as usize).wrapping_rem(PGSIZE) != 0 || pa < end.as_mut_ptr() || pa as usize >= PHYSTOP {
        panic!("kfree");
    }

    // Fill with junk to catch dangling refs.
    ptr::write_bytes(pa, 1, PGSIZE);
    let mut r = pa as *mut Run;
    let mut freelist = KMEM.lock();
    (*r).next = *freelist;
    *freelist = r;
}

/// Allocate one 4096-byte page of physical memory.
/// Returns a pointer that the kernel can use.
/// Returns 0 if the memory cannot be allocated.
pub unsafe fn kalloc() -> *mut u8 {
    let ret = {
        let mut freelist = KMEM.lock();
        if freelist.is_null() {
            return ptr::null_mut();
        }
        let next = (**freelist).next;
        mem::replace(&mut *freelist, next) as _
    };

    // fill with junk
    ptr::write_bytes(ret, 5, PGSIZE);
    ret
}
