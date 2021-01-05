//! Doubly circular intrusive linked list with head node.
use core::ptr;

pub struct ListEntry {
    pub next: *mut ListEntry,
    pub prev: *mut ListEntry,
}

impl ListEntry {
    pub const fn new() -> Self {
        Self {
            prev: ptr::null_mut(),
            next: ptr::null_mut(),
        }
    }
    
    pub fn init(&mut self) {
        self.next = self;
        self.prev = self;
    }

    /// # Safety
    ///
    /// Use only with initialized `ListEntry` types.
    pub unsafe fn next(&self) -> &mut Self {
        &mut *self.next
    }

    /// # Safety
    ///
    /// Use only with initialized `ListEntry` types.
    pub unsafe fn append(&mut self, e: &mut ListEntry) {
        e.next = self;
        e.prev = self.prev;

        (*e.next).prev = e;
        (*e.prev).next = e;
    }

    /// # Safety
    ///
    /// Use only with initialized `ListEntry` types.
    pub unsafe fn prepend(&mut self, e: &mut ListEntry) {
        e.next = self.next;
        e.prev = self;

        (*e.next).prev = e;
        (*e.prev).next = e;
    }

    pub fn is_empty(&self) -> bool {
        self.next as *const _ == self as *const _
    }

    /// # Safety
    ///
    /// Use only with initialized `ListEntry` types.
    pub unsafe fn remove(&mut self) {
        (*self.prev).next = self.next;
        (*self.next).prev = self.prev;
        self.init();
    }

    /// # Safety
    ///
    /// Use only with initialized `ListEntry` types.
    pub unsafe fn list_pop_front(&self) -> &ListEntry {
        let result = &mut *self.next;
        result.remove();
        result
    }
}