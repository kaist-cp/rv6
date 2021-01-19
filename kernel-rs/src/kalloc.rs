//! Physical memory allocator, for user processes,
//! kernel stacks, page-table pages,
//! and pipe buffers. Allocates whole 4096-byte pages.
use crate::{
    memlayout::PHYSTOP,
    page::Page,
    riscv::{pgrounddown, pgroundup, PGSIZE},
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

    /// Create pages between `end` and `PHYSTOP`.
    ///
    /// # Safety
    ///
    /// There must be no existing pages. It implies that this method should be
    /// called only once.
    pub unsafe fn init(&mut self) {
        // It is safe to acquire only the address of a static variable.
        let pa_start = pgroundup(unsafe { end.as_ptr() as usize });
        let pa_end = pgrounddown(PHYSTOP);
        for pa in num_iter::range_step(pa_start, pa_end, PGSIZE) {
            // It is safe because
            // * pa_start is a multiple of PGSIZE, and pa is so
            // * end <= pa < PHYSTOP
            // * the safety condition of this method guarantees that the
            //   created page does not overlap with existing pages
            self.free(unsafe { Page::from_usize(pa) });
        }
    }

    pub fn free(&mut self, pa: Page) {
        let pa = pa.into_usize();
        debug_assert!(
            // It is safe to acquire only the address of a static variable.
            pa % PGSIZE == 0 && (unsafe { end.as_ptr() as usize }..PHYSTOP).contains(&pa),
            "Kmem::free"
        );
        let mut r = pa as *mut Run;
        // By the invariant of Page, it does not create a cycle in this list and
        // thus is safe.
        unsafe { (*r).next = self.head };
        self.head = r;
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
