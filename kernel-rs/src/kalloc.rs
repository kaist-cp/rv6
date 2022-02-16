//! Physical memory allocator, for user processes,
//! kernel stacks, page-table pages,
//! and pipe buffers. Allocates whole 4096-byte pages.
use core::{mem, pin::Pin};

use pin_project::pin_project;

use crate::{
    addr::{pgrounddown, pgroundup, PGSIZE},
    lock::SpinLock,
    memlayout::PHYSTOP,
    page::Page,
    util::intrusive_list::{List, ListEntry, ListNode},
};

extern "C" {
    // first address after kernel.
    // defined by kernel.ld.
    pub static mut end: [u8; 0];
}

#[repr(transparent)]
#[pin_project]
struct Run {
    #[pin]
    entry: ListEntry,
}

impl Run {
    /// # Safety
    ///
    /// It must be used only after initializing it with `Run::init`.
    unsafe fn new() -> Self {
        Self {
            entry: unsafe { ListEntry::new() },
        }
    }

    fn init(self: Pin<&mut Self>) {
        self.project().entry.init();
    }
}

// SAFETY: `Run` owns a `ListEntry`.
unsafe impl ListNode for Run {
    fn get_list_entry(self: Pin<&mut Self>) -> Pin<&mut ListEntry> {
        self.project().entry
    }

    fn from_list_entry(list_entry: *mut ListEntry) -> *mut Self {
        list_entry as _
    }
}

/// # Safety
///
/// The address of each `Run` in `runs` can become a `Page` by `Page::from_usize`.
// This implementation defers from xv6. Kmem of xv6 uses intrusive singly linked list, while this
// Kmem uses List, which is a intrusive doubly linked list type of rv6. In a intrusive singly
// linked list, it is impossible to automatically remove an entry from a list when it is dropped.
// Therefore, it is nontrivial to make a general intrusive singly linked list type in a safe way.
// For this reason, we use a doubly linked list instead. It adds runtime overhead, but the overhead
// seems negligible.
#[pin_project]
pub struct Kmem {
    #[pin]
    runs: List<Run>,
}

impl Kmem {
    /// # Safety
    ///
    /// It must be used only after initializing it with `Kmem::init`.
    pub const unsafe fn new() -> Self {
        Self {
            runs: unsafe { List::new() },
        }
    }

    /// Create pages between `end` and `PHYSTOP`.
    ///
    /// # Safety
    ///
    /// There must be no existing pages. It implies that this method should be
    /// called only once.
    pub unsafe fn init(mut self: Pin<&mut Self>) {
        self.as_mut().project().runs.init();

        // SAFETY: safe to acquire only the address of a static variable.
        let pa_start = pgroundup(unsafe { end.as_ptr() as usize });
        let pa_end = pgrounddown(PHYSTOP);
        for pa in num_iter::range_step(pa_start, pa_end, PGSIZE) {
            // SAFETY:
            // * pa_start is a multiple of PGSIZE, and pa is so
            // * end <= pa < PHYSTOP
            // * the safety condition of this method guarantees that the
            //   created page does not overlap with existing pages
            self.as_mut().free(unsafe { Page::from_usize(pa) });
        }
    }

    pub fn free(self: Pin<&mut Self>, mut page: Page) {
        let run = page.as_uninit_mut();
        // SAFETY: `run` will be initialized by the following `init`.
        let run = run.write(unsafe { Run::new() });
        let mut run = unsafe { Pin::new_unchecked(run) };
        run.as_mut().init();
        self.project().runs.push_front(run);

        // Since the page has returned to the list, forget the page.
        mem::forget(page);
    }

    pub fn alloc(self: Pin<&mut Self>) -> Option<Page> {
        let run = self.project().runs.pop_front()?;
        // SAFETY: the invariant of `Kmem`.
        let page = unsafe { Page::from_usize(run as _) };
        Some(page)
    }
}

impl SpinLock<Kmem> {
    pub fn free(self: Pin<&Self>, mut page: Page) {
        // Fill with junk to catch dangling refs.
        page.write_bytes(1);
        self.pinned_lock().get_pin_mut().free(page);
    }

    pub fn alloc(self: Pin<&Self>, init_value: Option<u8>) -> Option<Page> {
        let mut page = self.pinned_lock().get_pin_mut().alloc()?;

        // fill with junk or received init value
        let init_value = init_value.unwrap_or(5);
        page.write_bytes(init_value);
        Some(page)
    }
}
