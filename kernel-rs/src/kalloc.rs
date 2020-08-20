//! Physical memory allocator, for user processes,
//! kernel stacks, page-table pages,
//! and pipe buffers. Allocates whole 4096-byte pages.
use crate::libc;
use crate::{
    memlayout::PHYSTOP,
    riscv::{pgroundup, PGSIZE},
    spinlock::Spinlock,
};
use core::{ops::DerefMut, ptr};

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

static mut KMEM: Spinlock<*mut Run> = Spinlock::new("KMEM", ptr::null_mut());

pub unsafe fn kinit() {
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
        panic!("kfree");
    }

    // Fill with junk to catch dangling refs.
    ptr::write_bytes(pa as *mut libc::CVoid, 1, PGSIZE);
    let mut r: *mut Run = pa as *mut Run;
    let mut freelist = KMEM.lock();
    (*r).next = *(freelist.deref_mut()) as *mut Run;
    *(freelist.deref_mut()) = r;
}

/// Allocate one 4096-byte page of physical memory.
/// Returns a pointer that the kernel can use.
/// Returns 0 if the memory cannot be allocated.
pub unsafe fn kalloc() -> *mut libc::CVoid {
    let mut freelist = KMEM.lock();
    let data = freelist.deref_mut();
    let r = *data;
    if !data.is_null() {
        *data = (*(*data)).next;
    }
    drop(freelist);
    if !r.is_null() {
        // fill with junk
        ptr::write_bytes(r as *mut u8 as *mut libc::CVoid, 5, PGSIZE);
    }
    r as *mut libc::CVoid
}
