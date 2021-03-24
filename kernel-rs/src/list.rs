//! Doubly intrusive linked list with head node.
//! A `List` or `ListEntry` must be first initialized before using its methods.
//!
//! # Lifetime-less intrusive linked lists
//!
//! Intrusive linked lists are interesting and useful because the list does not own the nodes.
//! However, it can also be unsafe if the nodes could move or drop while it's inside the list.
//! Hence, many intrusive linked lists written in Rust use lifetimes and prohibit nodes
//! from being moved or dropped during the list's whole lifetime.
//! However, this means nodes cannot be mutated, moved or dropped even after it was removed from the list.
//!
//! On contrast, this list does not use lifetimes and allows nodes from being mutated or dropped
//! even when its inside the list. When a node gets dropped, we simply remove it from the list.
//! Instead, a `List` or `ListEntry`'s methods never returns a reference to a node or `ListEntry`, and always
//! returns a raw pointer instead. This is because a node could get mutated or dropped at any time, and hence,
//! the caller should make sure the node is not under mutation or already dropped when dereferencing the raw pointer.
// TODO: Also allow move.

use core::cell::Cell;
use core::marker::PhantomPinned;
use core::pin::Pin;
use core::ptr;

use pin_project::{pin_project, pinned_drop};

/// A doubly linked list.
/// Can only contain types that implement the `ListNode` trait.
/// Use only after initialization.
///
/// # Safety
///
/// A `List` contains one or more `ListEntry`s.
/// * Exactly one of them is the `head`.
/// * All other of them are a `ListEntry` owned by a `T: ListNode`.
#[pin_project(PinnedDrop)]
pub struct List<T: ListNode> {
    #[pin]
    head: ListEntry<T>,
}

/// An iterator over the elements of `List`.
pub struct Iter<'s, T: ListNode> {
    last: &'s ListEntry<T>,
    curr: &'s ListEntry<T>,
}

/// Intrusive linked list nodes that can be inserted into a `List`.
///
/// # Safety
///
/// Only implement this for structs that own a `ListEntry`.
/// The required functions should provide conversion between the struct and its `ListEntry`.
pub unsafe trait ListNode: Sized {
    /// Returns a reference of this struct's `ListEntry`.
    fn get_list_entry(&self) -> &ListEntry<Self>;

    /// Returns a raw pointer which points to the struct that owns the given `list_entry`.
    /// You may want to use `offset_of!` to implement this.
    fn from_list_entry(list_entry: *const ListEntry<Self>) -> *const Self;
}

/// A list entry for doubly, intrusive linked lists.
///
/// # Safety
///
/// * All `ListEntry` types must be used only after initializing it with `ListEntry::init`.
/// After this, `ListEntry::{prev, next}` always refer to a valid, initialized `ListEntry`.
#[pin_project(PinnedDrop)]
pub struct ListEntry<T: ListNode> {
    prev: Cell<*const Self>,
    next: Cell<*const Self>,
    #[pin]
    _marker: PhantomPinned, //`ListEntry` is `!Unpin`.
}

impl<T: ListNode> List<T> {
    /// Returns an uninitialized `List`,
    ///
    /// # Safety
    ///
    /// All `List` types must be used only after initializing it with `List::init`.
    pub const unsafe fn new() -> Self {
        Self {
            head: unsafe { ListEntry::new() },
        }
    }

    /// Initializes this `ListEntry` if it was not initialized.
    /// Otherwise, does nothing.
    pub fn init(self: Pin<&mut Self>) {
        self.project().head.init();
    }

    /// Returns true if this `List` is empty.
    /// Otherwise, returns flase.
    pub fn is_empty(&self) -> bool {
        self.head.is_unlinked()
    }

    /// Provides a raw pointer to the back node, or `None` if the list is empty.
    pub fn back(&self) -> Option<*const T> {
        if self.is_empty() {
            None
        } else {
            Some(T::from_list_entry(self.head.prev()))
        }
    }

    /// Provides a raw pointer to the front node, or `None` if the list is empty.
    pub fn front(&self) -> Option<*const T> {
        if self.is_empty() {
            None
        } else {
            Some(T::from_list_entry(self.head.next()))
        }
    }

    /// Push `elt` at the back of the list after unlinking it.
    // TODO: Use PinFreeze<T>?
    pub fn push_back(&self, elt: &T) {
        self.head.push_back(elt);
    }

    /// Push `elt` at the front of the list after unlinking it.
    pub fn push_front(&self, elt: &T) {
        self.head.push_front(elt);
    }

    /// Removes the last node from the list and returns a raw pointer to it,
    /// or `None` if the list is empty.
    pub fn pop_back(&self) -> Option<*const T> {
        let ptr = self.head.prev();
        if ptr::eq(ptr, &self.head) {
            None
        } else {
            unsafe { (&*ptr).remove() };
            Some(T::from_list_entry(ptr))
        }
    }

    /// Removes the last node from the list and returns a raw pointer to it,
    /// or `None` if the list is empty.
    pub fn pop_front(&self) -> Option<*const T> {
        let ptr = self.head.next();
        if ptr::eq(ptr, &self.head) {
            None
        } else {
            unsafe { (&*ptr).remove() };
            Some(T::from_list_entry(ptr))
        }
    }

    /// Removes all nodes from the list.
    pub fn clear(&self) {
        while self.pop_front().is_some() {}
    }

    /// Provides an unsafe forward iterator.
    ///
    /// # Safety
    ///
    /// The caller must make sure that the iterator's **current** items does not get removed, mutated, or dropped.
    /// * If the item gets removed, the iterator will loop forever.
    /// * If the item gets mutated/dropped, using the iterator may lead to undefined behavior.
    pub unsafe fn unsafe_iter(&self) -> Iter<'_, T> {
        Iter {
            last: &self.head,
            curr: unsafe { &*self.head.next() },
        }
    }
}

#[pinned_drop]
impl<T: ListNode> PinnedDrop for List<T> {
    fn drop(self: Pin<&mut Self>) {
        self.clear();
    }
}

impl<'s, T: 's + ListNode> Iterator for Iter<'s, T> {
    type Item = &'s T;

    fn next(&mut self) -> Option<Self::Item> {
        if ptr::eq(self.last, self.curr) {
            None
        } else {
            // Safe since `self.curr` is a `ListEntry` contained inside a `T`.
            let res = Some(unsafe { &*T::from_list_entry(self.curr) });
            debug_assert_ne!(self.curr as *const _, self.curr.next(), "loops forever");
            self.curr = unsafe { &*self.curr.next() };
            res
        }
    }
}

impl<'s, T: 's + ListNode> DoubleEndedIterator for Iter<'s, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if ptr::eq(self.last, self.curr) {
            None
        } else {
            debug_assert_ne!(self.last as *const _, self.last.prev(), "loops forever");
            self.last = unsafe { &*self.last.prev() };
            // Safe since `self.last` is a `ListEntry` contained inside a `T`.
            Some(unsafe { &*T::from_list_entry(self.last) })
        }
    }
}

impl<T: ListNode> ListEntry<T> {
    /// Returns an uninitialized `ListEntry`,
    ///
    /// # Safety
    ///
    /// All `ListEntry` types must be used only after initializing it with `ListEntry::init`.
    pub const unsafe fn new() -> Self {
        Self {
            prev: Cell::new(ptr::null_mut()),
            next: Cell::new(ptr::null_mut()),
            _marker: PhantomPinned,
        }
    }

    /// Initializes this `ListEntry` if it was not initialized.
    /// Otherwise, does nothing.
    pub fn init(self: Pin<&mut Self>) {
        if self.next().is_null() {
            self.next.set(self.as_ref().get_ref());
            self.prev.set(self.as_ref().get_ref());
        }
    }

    /// Returns a raw pointer pointing to the previous `ListEntry`.
    ///
    /// # Note
    ///
    /// Do not use `ListNode::from_list_entry` on the returned pointer if `self` is the front node of a list.
    pub fn prev(&self) -> *const Self {
        self.prev.get()
    }

    /// Returns a raw pointer pointing to the next `ListEntry`.
    ///
    /// # Note
    ///
    /// Do not use `ListNode::from_list_entry` on the returned pointer if `self` is the back node of a list.
    pub fn next(&self) -> *const Self {
        self.next.get()
    }

    /// Returns `true` if this `ListEntry` is not linked to any other `ListEntry`.
    /// Otherwise, returns `false`.
    pub fn is_unlinked(&self) -> bool {
        ptr::eq(self.next(), self)
    }

    /// Inserts `elt` at the back of this `ListEntry` after unlinking `elt`.
    pub fn push_back(&self, elt: &T) {
        let e = elt.get_list_entry();
        if !e.is_unlinked() {
            e.remove();
        }

        e.next.set(self);
        e.prev.set(self.prev());
        unsafe {
            (*e.next()).prev.set(e);
            (*e.prev()).next.set(e);
        }
    }

    /// Inserts `elt` in front of this `ListEntry` after unlinking `elt`.
    pub fn push_front(&self, elt: &T) {
        let e = elt.get_list_entry();
        if !e.is_unlinked() {
            e.remove();
        }

        e.next.set(self.next());
        e.prev.set(self);
        unsafe {
            (*e.next()).prev.set(e);
            (*e.prev()).next.set(e);
        }
    }

    /// Unlinks this `ListEntry` from other `ListEntry`s.
    pub fn remove(&self) {
        unsafe {
            (*self.prev()).next.set(self.next());
            (*self.next()).prev.set(self.prev());
        }
        self.prev.set(self);
        self.next.set(self);
    }
}

#[pinned_drop]
impl<T: ListNode> PinnedDrop for ListEntry<T> {
    fn drop(self: Pin<&mut Self>) {
        self.remove();
    }
}
