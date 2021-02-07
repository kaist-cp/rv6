//! Doubly circular intrusive linked list with head node.
//! `ListEntry` types must be first initialized with init()
//! before calling its member functions.

use crate::pincell::WeakPin;
use core::marker::PhantomPinned;
use core::pin::Pin;
use pin_project::{pin_project, pinned_drop};

#[pin_project(PinnedDrop)]
pub struct ListEntry {
    next: WeakPin<*mut Self>,
    prev: WeakPin<*mut Self>,
    #[pin]
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
            prev: unsafe { WeakPin::zero() },
            next: unsafe { WeakPin::zero() },
            _marker: PhantomPinned,
        }
    }

    pub fn set_prev(mut self: WeakPin<*mut Self>, prev: WeakPin<*mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        this.prev = prev;
    }

    pub fn set_next(mut self: WeakPin<*mut Self>, next: WeakPin<*mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        this.next = next;
    }

    pub fn init(mut self: Pin<&mut Self>) {
        // Safe since we don't move the inner data and don't leak the mutable reference.
        let weak = WeakPin::from_pin(self.as_mut());
        let this = unsafe { self.get_unchecked_mut() };
        this.next = weak;
        this.prev = weak;
    }

    pub fn prev(&self) -> &Self {
        &*self.prev
    }

    pub fn next(&self) -> &Self {
        &*self.next
    }

    /// `e` <-> `this`
    pub fn append(mut self: Pin<&mut Self>, mut e: Pin<&mut Self>) {
        // Safe since we don't move the inner data and don't leak the mutable reference.
        let this = WeakPin::from_pin(self.as_mut());
        let elem = WeakPin::from_pin(e.as_mut());

        elem.set_next(this);
        elem.set_prev(this.prev);
        elem.next.set_prev(elem);
        elem.prev.set_next(elem);
    }

    /// `this` <-> `e`
    pub fn prepend(mut self: Pin<&mut Self>, mut e: Pin<&mut Self>) {
        // Safe since we don't move the inner data and don't leak the mutable reference.
        let this = WeakPin::from_pin(self.as_mut());
        let elem = WeakPin::from_pin(e.as_mut());

        elem.set_next(this.next);
        elem.set_prev(this);
        elem.next.set_prev(elem);
        elem.prev.set_next(elem);
    }

    // pub fn is_empty(self: Pin<&Self>) -> bool {
    //     let this = WeakPin::from_pin(self);
    //     this.next == this
    // }

    pub fn remove(mut self: Pin<&mut Self>) {
        let this = WeakPin::from_pin(self.as_mut());
        this.prev.set_next(this.next);
        this.next.set_prev(this.prev);
        self.init();
    }

    // not used
    // pub fn list_pop_front(self: Pin<&mut Self>) -> Pin<&mut Self> {
    //     // Safe since we don't move the inner data and don't leak the mutable reference.
    //     let mut result = unsafe { Pin::new_unchecked(&mut *self.next) };
    //     result.as_mut().remove();
    //     result
    // }
}

#[pinned_drop]
impl PinnedDrop for ListEntry {
    fn drop(self: Pin<&mut Self>) {
        self.remove();
    }
}
