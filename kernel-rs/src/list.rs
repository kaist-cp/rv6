//! Doubly circular intrusive linked list with head node.
//! `ListEntry` types must be first initialized with init()
//! before calling its member functions.

use core::ptr;

pub struct ListEntry {
    next: *mut ListEntry,
    prev: *mut ListEntry,
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

    pub fn prev(&self) -> &Self {
        unsafe { &*self.prev }
    }

    pub fn next(&self) -> &Self {
        unsafe { &*self.next }
    }

    /// `e` <-> `this`
    pub fn append(&mut self, e: &mut ListEntry) {
        e.next = self;
        e.prev = self.prev;

        unsafe {
            (*e.next).prev = e;
            (*e.prev).next = e;
        }
    }

    /// `this` <-> `e`
    pub fn prepend(&mut self, e: &mut ListEntry) {
        e.next = self.next;
        e.prev = self;

        unsafe {
            (*e.next).prev = e;
            (*e.prev).next = e;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.next as *const _ == self as *const _
    }

    pub fn remove(&mut self) {
        unsafe {
            (*self.prev).next = self.next;
            (*self.next).prev = self.prev;
        }
        self.init();
    }

    pub fn list_pop_front(&self) -> &ListEntry {
        let result = unsafe { &mut *self.next };
        result.remove();
        result
    }
}
