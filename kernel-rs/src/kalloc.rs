//! Physical memory allocator, for user processes,
//! kernel stacks, page-table pages,
//! and pipe buffers. Allocates whole 4096-byte pages.
use crate::{
    memlayout::PHYSTOP,
    riscv::{pgroundup, PGSIZE},
};

use core::mem;
use core::ptr;

extern "C" {
    // first address after kernel.
    // defined by kernel.ld.
    #[no_mangle]
    pub static mut end: [u8; 0];
}

struct Run {
    next: *mut Run,
}

pub struct Kmem {
    head: *mut Run,
}

impl Kmem {
    pub const fn new() -> Self {
        Self {
            head: ptr::null_mut(),
        }
    }

    pub unsafe fn free(&mut self, pa: *mut u8) {
        let mut r = pa as *mut Run;
        (*r).next = self.head;
        self.head = r;
    }

    pub unsafe fn freerange(&mut self, pa_start: *mut u8, pa_end: *mut u8) {
        let mut p = pgroundup(pa_start as _) as *mut u8;
        while p.add(PGSIZE) <= pa_end {
            self.free(p);
            p = p.add(PGSIZE);
        }
    }

    pub unsafe fn alloc(&mut self) -> *mut u8 {
        if self.head.is_null() {
            return ptr::null_mut();
        }
        let next = (*self.head).next;
        mem::replace(&mut self.head, next) as _
    }
}

pub unsafe fn kinit(kmem: &mut Kmem) {
    kmem.freerange(end.as_mut_ptr(), PHYSTOP as _);
}
