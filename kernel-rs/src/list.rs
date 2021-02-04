//! Doubly circular intrusive linked list with head node.
//! `ListEntry` types must be first initialized with init()
//! before calling its member functions.

use core::marker::PhantomPinned;
use core::pin::Pin;
use core::ptr;
use pin_project::{pin_project, pinned_drop};

#[pin_project(PinnedDrop)]
pub struct ListEntry {
    next: *mut ListEntry,
    prev: *mut ListEntry,
    _marker: PhantomPinned, //`ListEntry` is `!Unpin`.
}

/// A list entry for doubly, circular, intrusive linked lists.
///
/// # Safety
///
/// All `ListEntry` types must be used only after initializing it with `ListEntry::init()`,
/// or after appending/prepending it to another initialized `ListEntry`.
/// After this, `ListEntry::{prev, next}` always refer to a valid, initialized `ListEntry`.
impl ListEntry {
    /// Returns an uninitialized `ListEntry`,
    ///
    /// # Safety
    ///
    /// All `ListEntry` types must be used only after initializing it with `ListEntry::init()`,
    /// or after appending/prepending it to another initialized `ListEntry`.
    pub const unsafe fn new() -> Self {
        Self {
            prev: ptr::null_mut(),
            next: ptr::null_mut(),
            _marker: PhantomPinned,
        }
    }

    pub fn init(self: Pin<&mut Self>) {
        // Safe since we don't move the inner data and don't leak the mutable reference.
        let this = unsafe { self.get_unchecked_mut() };
        this.next = this;
        this.prev = this;
    }

    pub fn prev(&self) -> &Self {
        unsafe { &*self.prev }
    }

    pub fn next(&self) -> &Self {
        unsafe { &*self.next }
    }

    /// `e` <-> `this`
    pub fn append(self: Pin<&mut Self>, e: Pin<&mut Self>) {
        // Safe since we don't move the inner data and don't leak the mutable reference.
        let this = unsafe { self.get_unchecked_mut() };
        let elem = unsafe { e.get_unchecked_mut() };

        elem.next = this;
        elem.prev = this.prev;
        unsafe {
            (*elem.next).prev = elem;
            (*elem.prev).next = elem;
        }
    }

    /// `this` <-> `e`
    pub fn prepend(self: Pin<&mut Self>, e: Pin<&mut Self>) {
        // Safe since we don't move the inner data and don't leak the mutable reference.
        let this = unsafe { self.get_unchecked_mut() };
        let elem = unsafe { e.get_unchecked_mut() };

        elem.next = this.next;
        elem.prev = this;
        unsafe {
            (*elem.next).prev = elem;
            (*elem.prev).next = elem;
        }
    }

    pub fn is_empty(&self) -> bool {
        ptr::eq(self.next, self)
    }

    pub fn remove(mut self: Pin<&mut Self>) {
        unsafe {
            (*self.prev).next = self.next;
            (*self.next).prev = self.prev;
        }
        self.init();
    }

    pub fn list_pop_front(mut self: Pin<&mut Self>) -> Pin<&mut Self> {
        // Safe since we don't move the inner data and don't leak the mutable reference.
        let mut result = unsafe { Pin::new_unchecked(&mut *self.next) };
        result.as_mut().remove();
        result
    }
}

#[pinned_drop]
impl PinnedDrop for ListEntry {
    fn drop(self: Pin<&mut Self>) {
        self.remove();
    }
}
