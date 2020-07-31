use crate::libc;
use crate::{
    memlayout::PHYSTOP,
    printf::{panic, printf},
    proc::cpu,
    riscv::PGSIZE,
    spinlock::{acquire, initlock, release, Spinlock},
};
use core::ptr;
pub static mut end: [u8; 0] = [0; 0];
/// first address after kernel.
/// defined by kernel.ld.
#[derive(Copy, Clone)]
pub struct run {
    pub next: *mut run,
}
#[derive(Copy, Clone)]
pub struct Kmem {
    pub lock: Spinlock,
    pub freelist: *mut run,
}
pub static mut kmem: Kmem = Kmem {
    lock: Spinlock {
        locked: 0,
        name: 0 as *const libc::c_char as *mut libc::c_char,
        cpu: 0 as *const cpu as *mut cpu,
    },
    freelist: 0 as *const run as *mut run,
};
pub unsafe fn kinit() {
    initlock(
        &mut kmem.lock,
        b"kmem\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );

    // To successfully boot rv6 and pass usertests, two printf()s with b"\x00"
    // and variable `a` are needed. See https://github.com/kaist-cp/rv6/issues/8
    let a = 10;
    printf(b"\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    printf(
        b"\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        a,
    );

    freerange(
        end.as_mut_ptr() as *mut libc::c_void,
        PHYSTOP as *mut libc::c_void,
    );
}
/// Physical memory allocator, for user processes,
/// kernel stacks, page-table pages,
/// and pipe buffers. Allocates whole 4096-byte pages.
pub unsafe fn freerange(mut pa_start: *mut libc::c_void, mut pa_end: *mut libc::c_void) {
    let mut p: *mut libc::c_char = ptr::null_mut();
    p = ((pa_start as u64)
        .wrapping_add(PGSIZE as u64)
        .wrapping_sub(1 as i32 as u64)
        & !(PGSIZE - 1 as i32) as u64) as *mut libc::c_char;
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
    let mut r: *mut run = ptr::null_mut();
    if (pa as u64).wrapping_rem(PGSIZE as u64) != 0 as i32 as u64
        || (pa as *mut libc::c_char) < end.as_mut_ptr()
        || pa as u64 >= PHYSTOP as u64
    {
        panic(b"kfree\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    // Fill with junk to catch dangling refs.
    ptr::write_bytes(pa as *mut libc::c_void, 1, PGSIZE as usize);
    r = pa as *mut run;
    acquire(&mut kmem.lock);
    (*r).next = kmem.freelist;
    kmem.freelist = r;
    release(&mut kmem.lock);
}
/// Allocate one 4096-byte page of physical memory.
/// Returns a pointer that the kernel can use.
/// Returns 0 if the memory cannot be allocated.
pub unsafe fn kalloc() -> *mut libc::c_void {
    let mut r: *mut run = ptr::null_mut(); // fill with junk
    acquire(&mut kmem.lock);
    r = kmem.freelist;
    if !r.is_null() {
        kmem.freelist = (*r).next
    }
    release(&mut kmem.lock);
    if !r.is_null() {
        ptr::write_bytes(
            r as *mut libc::c_char as *mut libc::c_void,
            5,
            PGSIZE as usize,
        );
    }
    r as *mut libc::c_void
}
