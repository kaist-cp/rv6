//! Physical memory allocator, for user processes,
//! kernel stacks, page-table pages,
//! and pipe buffers. Allocates whole 4096-byte pages.
use crate::{
    memlayout::PHYSTOP,
    page::Page,
    riscv::{pgroundup, PGSIZE},
};

use core::mem;
use core::ptr;

extern "C" {
    // first address after kernel.
    // defined by kernel.ld.
    pub static mut end: [u8; 0];
}

struct Run {
    next: *mut Run,
}

/// # Safety
///
/// - This singly linked list does not have a cycle.
/// - If head is null, then it is an empty list. Ohterwise, it is nonempty, and
///   head is its first element, which is a valid page.
pub struct Kmem {
    head: *mut Run,
}

impl Kmem {
    pub const fn new() -> Self {
        Self {
            head: ptr::null_mut(),
        }
    }

    /// # Safety
    ///
    /// Must be called only once. Create pages between `end` and `PHYSTOP` by
    /// calling freerange.
    pub unsafe fn init(&mut self) {
        self.freerange(end.as_mut_ptr(), PHYSTOP as _);
    }

    pub fn free(&mut self, pa: Page) {
        let mut r = pa.into_usize() as *mut Run;
        // By the invariant of Page, it does not create a cycle in this list and
        // thus is safe.
        unsafe { (*r).next = self.head };
        self.head = r;
    }

    /// # Safety
    ///
    /// Create pages between `pa_start` and `pa_end`. Created pages must
    /// not overwrap with any existing pages.
    unsafe fn freerange(&mut self, pa_start: *mut u8, pa_end: *mut u8) {
        let mut p = pgroundup(pa_start as _) as *mut u8;
        while p.add(PGSIZE) <= pa_end {
            self.free(Page::from_usize(p as _));
            p = p.add(PGSIZE);
        }
    }

    pub fn alloc(&mut self) -> Option<Page> {
        if self.head.is_null() {
            return None;
        }
        // It is safe because head is not null and the structure of this list
        // is maintained by the invariant.
        let next = unsafe { (*self.head).next };
        // It is safe because the first element is a valid page by the invariant.
        Some(unsafe { Page::from_usize(mem::replace(&mut self.head, next) as _) })
    }
}
