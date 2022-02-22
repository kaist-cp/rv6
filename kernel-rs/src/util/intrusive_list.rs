//! Doubly intrusive linked list with head node.
//! A [`List`] or [`ListEntry`] must be first initialized before using its methods.
//!
//! # Lifetime-less intrusive linked lists
//!
//! Intrusive linked lists are interesting and useful because the list does not own the nodes.
//! However, it can also be unsafe if the nodes could move or drop while it's inserted in the list.
//! Hence, many intrusive linked lists written in Rust use lifetimes and prohibit nodes
//! from being moved or dropped during the list's whole lifetime.
//! However, this means nodes cannot be mutated, moved or dropped even after it was removed from the list.
//!
//! In contrast, [`List`] does not use lifetimes and allows nodes from being mutated or dropped
//! even when its inserted in the list. When a node gets dropped, we simply remove it from the list.
//! Instead, a [`List`] or [`ListEntry`]'s methods never returns a reference to a node or [`ListEntry`], and always
//! returns a raw pointer instead. This is because a node could get mutated or dropped at any time, and hence,
//! the caller should make sure the node is not under mutation or already dropped when dereferencing the raw pointer.
// TODO: Need to add interior mutability? Otherwise, UB?
// TODO: Also allow move.

use core::marker::{PhantomData, PhantomPinned};
use core::pin::Pin;
use core::ptr;

use pin_project::{pin_project, pinned_drop};

use super::strong_pin::StrongPinMut;

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
    head: ListEntry,
    _marker: PhantomData<T>,
}

/// An iterator over the elements of `List`.
///
/// # Safety
///
/// * There are no `Pin<&mut ListNode>` for any `ListNode` inside the `List`, while the `Iter` exists.
/// * `last` and `curr` always points to a valid `ListEntry`.
pub struct Iter<'s, T: ListNode> {
    last: *const ListEntry,
    curr: *const ListEntry,
    _marker: PhantomData<&'s T>,
}

pub struct IterStrongPinMut<'s, T> {
    last: *mut ListEntry,
    curr: *mut ListEntry,
    _marker: PhantomData<&'s mut T>,
}

/// A pinned mutable iterator over the elements of `List`.
///
/// # Safety
///
/// * There are no `&ListNode` or `Pin<&mut ListNode>` for any `ListNode` inside the `List`,
/// while the `IterPinMut` exists.
/// * `last` and `curr` always points to a valid `ListEntry`.
pub struct IterPinMut<'s, T: ListNode> {
    last: *mut ListEntry,
    curr: *mut ListEntry,
    _marker: PhantomData<&'s mut T>,
}

/// Intrusive linked list nodes that can be inserted into a `List`.
///
/// # Safety
///
/// Only implement this for structs that own a `ListEntry`.
/// The required functions should provide conversion between the struct and its `ListEntry`.
pub unsafe trait ListNode: Sized {
    /// Returns a reference of this struct's `ListEntry`.
    fn get_list_entry(self: Pin<&mut Self>) -> Pin<&mut ListEntry>;

    /// Returns a raw pointer which points to the struct that owns the given `list_entry`.
    /// You may want to use `offset_of!` to implement this.
    fn from_list_entry(list_entry: *mut ListEntry) -> *mut Self;
}

/// A low level primitive for doubly, intrusive linked lists and nodes.
///
/// # Safety
///
/// * All `ListEntry` types must be used only after initializing it with `ListEntry::init`.
/// After this, `ListEntry::{prev, next}` always refer to a valid, initialized `ListEntry`.
#[pin_project(PinnedDrop)]
pub struct ListEntry {
    prev: *mut Self,
    next: *mut Self,
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
            _marker: PhantomData,
        }
    }

    /// Initializes this `ListEntry` if it was not initialized.
    /// Otherwise, does nothing.
    ///
    /// # Note
    ///
    /// Do not call this method more than once.
    pub fn init(self: Pin<&mut Self>) {
        self.project().head.init();
    }

    fn head(self: Pin<&Self>) -> Pin<&ListEntry> {
        self.project_ref().head
    }

    fn head_mut(self: Pin<&mut Self>) -> Pin<&mut ListEntry> {
        self.project().head
    }

    /// Returns true if this `List` is empty.
    /// Otherwise, returns flase.
    pub fn is_empty(self: Pin<&Self>) -> bool {
        self.head().is_unlinked()
    }

    /// Provides a raw pointer to the back node, or `None` if the list is empty.
    pub fn back(self: Pin<&Self>) -> Option<*mut T> {
        if self.is_empty() {
            None
        } else {
            Some(T::from_list_entry(self.head().prev()))
        }
    }

    /// Provides a raw pointer to the front node, or `None` if the list is empty.
    pub fn front(self: Pin<&Self>) -> Option<*mut T> {
        if self.is_empty() {
            None
        } else {
            Some(T::from_list_entry(self.head().next()))
        }
    }

    /// Push `elt` at the back of the list after unlinking it.
    pub fn push_back(self: Pin<&mut Self>, elt: Pin<&mut T>) {
        self.head_mut().push_back(elt.get_list_entry());
    }

    /// Push `elt` at the front of the list after unlinking it.
    pub fn push_front(self: Pin<&mut Self>, elt: Pin<&mut T>) {
        self.head_mut().push_front(elt.get_list_entry());
    }

    /// Removes the last node from the list and returns a raw pointer to it,
    /// or `None` if the list is empty.
    pub fn pop_back(self: Pin<&mut Self>) -> Option<*mut T> {
        let ptr = self.as_ref().head().prev();
        if ptr::eq(ptr, &self.head) {
            None
        } else {
            let mut prev = unsafe { Pin::new_unchecked(&mut *ptr) };
            prev.as_mut().remove();
            Some(T::from_list_entry(prev.as_ptr()))
        }
    }

    /// Removes the last node from the list and returns a raw pointer to it,
    /// or `None` if the list is empty.
    pub fn pop_front(self: Pin<&mut Self>) -> Option<*mut T> {
        let ptr = self.as_ref().head().next();
        if ptr::eq(ptr, &self.head) {
            None
        } else {
            let mut next = unsafe { Pin::new_unchecked(&mut *ptr) };
            next.as_mut().remove();
            Some(T::from_list_entry(next.as_ptr()))
        }
    }

    /// Removes all nodes from the list.
    pub fn clear(mut self: Pin<&mut Self>) {
        while self.as_mut().pop_front().is_some() {}
    }

    /// Provides an unsafe forward iterator.
    ///
    /// # Note
    ///
    /// The caller should be careful when removing nodes currently accessed by iterators.
    /// If an iterator's current node gets removed, the iterator will get stuck at the current node and never advance.
    ///
    /// # Safety
    ///
    /// The caller should be even more careful when mutating or dropping nodes that are currently
    /// accessed by iterators. This can lead to undefined behavior.
    ///
    /// # Examples
    ///
    /// *Incorrect* usage of this method.
    ///
    /// ```rust,no_run
    /// # #[pin_project]
    /// # struct Node {
    /// #     data: usize,
    /// #     #[pin]
    /// #     list_entry: ListEntry,
    /// # }
    /// #
    /// # unsafe impl ListNode for Node {
    /// #     fn get_list_entry(self: Pin<&mut Self>) -> Pin<&mut ListEntry> {
    /// #         self.project().list_entry
    /// #     }
    /// #
    /// #     fn from_list_entry(list_entry: *mut ListEntry) -> *mut Self {
    /// #         (list_entry as usize - 8) as *mut Self
    /// #     }
    /// # }
    /// #
    /// # fn main() {
    ///     // Make and initialize a `List` and a `Node` that implements the `ListNode` trait.
    ///     let mut list = unsafe { List::new() };
    ///     let mut list_pin = unsafe { Pin::new_unchecked(&mut list) };
    ///     list_pin.as_mut().init();
    ///
    ///     let mut node = Some(unsafe { Node { data: 10, list_entry: ListEntry::new() }});
    ///     let mut node_pin = unsafe { Pin::new_unchecked(node.as_mut().expect("")) };
    ///     node_pin.as_mut().project().list_entry.init();
    ///    
    ///     // Push the `ListNode` to the `List`.
    ///     list_pin.as_mut().push_front(node_pin);
    ///
    ///     // Use an unsafe iterator.
    ///     for n in unsafe { list_pin.as_ref().iter_unchecked() } {
    ///         assert!(n.data == 10);  // okay!
    ///         node = None;
    ///         assert!(n.data == 10);  // not okay! reading data of already dropped node!
    ///                                 // undefined behavior! ⚠️
    ///     }
    ///
    ///     assert!(node.is_none());
    /// # }
    /// ```
    pub unsafe fn iter_unchecked(self: Pin<&Self>) -> Iter<'_, T> {
        Iter {
            last: &self.head,
            curr: self.head().next(),
            _marker: PhantomData,
        }
    }

    #[allow(clippy::needless_lifetimes)]
    pub unsafe fn iter_strong_pin_mut_unchecked<'s>(
        self: StrongPinMut<'s, Self>,
    ) -> IterStrongPinMut<'s, T> {
        let last = unsafe { &raw mut (*self.ptr().as_ptr()).head };
        let curr = self.as_ref().as_pin().head().next();
        IterStrongPinMut {
            last,
            curr,
            _marker: PhantomData,
        }
    }

    /// Provides an unsafe, mutable forward iterator.
    /// See `List:iter_unchecked` for details.
    ///
    /// # Note
    ///
    /// The caller should be careful when removing nodes currently accessed by iterators.
    /// If an iterator's current node gets removed, the iterator will get stuck at the current node and never advance.
    ///
    /// # Safety
    ///
    /// The caller should be even more careful when accessing or dropping nodes that are currently
    /// accessed by iterators. This can lead to undefined behavior.
    pub unsafe fn iter_pin_mut_unchecked(mut self: Pin<&mut Self>) -> IterPinMut<'_, T> {
        IterPinMut {
            last: self.as_mut().head_mut().as_ptr(),
            curr: self.as_ref().head().next(),
            _marker: PhantomData,
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
            let ptr = T::from_list_entry(self.curr as *mut _) as *const T;
            let res = Some(unsafe { &*ptr });
            let curr = unsafe { Pin::new_unchecked(&*self.curr) };
            debug_assert_ne!(self.curr, curr.next(), "loops forever");
            self.curr = curr.next();
            res
        }
    }
}

impl<'s, T: 's + ListNode> DoubleEndedIterator for Iter<'s, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if ptr::eq(self.last, self.curr) {
            None
        } else {
            let last = unsafe { Pin::new_unchecked(&*self.last) };
            debug_assert_ne!(self.last, last.prev(), "loops forever");
            self.last = last.prev();
            // Safe since `self.last` is a `ListEntry` contained inside a `T`.
            let ptr = T::from_list_entry(self.last as *mut _) as *const T;
            Some(unsafe { &*ptr })
        }
    }
}

impl<'s, T: 's + ListNode> Iterator for IterStrongPinMut<'s, T> {
    type Item = StrongPinMut<'s, T>;

    fn next(&mut self) -> Option<Self::Item> {
        if ptr::eq(self.last, self.curr) {
            None
        } else {
            // Safe since `self.curr` is a `ListEntry` contained inside a `T`.
            let ptr = T::from_list_entry(self.curr);
            let res = Some(unsafe { StrongPinMut::new_unchecked(ptr) });
            let curr = unsafe { Pin::new_unchecked(&*self.curr) };
            debug_assert_ne!(self.curr, curr.next(), "loops forever");
            self.curr = curr.next();
            res
        }
    }
}

impl<'s, T: 's + ListNode> DoubleEndedIterator for IterStrongPinMut<'s, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if ptr::eq(self.last, self.curr) {
            None
        } else {
            let last = unsafe { Pin::new_unchecked(&*self.last) };
            debug_assert_ne!(self.last, last.prev(), "loops forever");
            self.last = last.prev();
            // Safe since `self.last` is a `ListEntry` contained inside a `T`.
            let ptr = T::from_list_entry(self.last);
            Some(unsafe { StrongPinMut::new_unchecked(ptr) })
        }
    }
}

impl<'s, T: 's + ListNode> Iterator for IterPinMut<'s, T> {
    type Item = Pin<&'s mut T>;

    fn next(&mut self) -> Option<Self::Item> {
        if ptr::eq(self.last, self.curr) {
            None
        } else {
            // Safe since `self.curr` is a `ListEntry` contained inside a `T`.
            let ptr = T::from_list_entry(self.curr);
            let res = Some(unsafe { Pin::new_unchecked(&mut *ptr) });
            let curr = unsafe { Pin::new_unchecked(&*self.curr) };
            debug_assert_ne!(self.curr, curr.next(), "loops forever");
            self.curr = curr.next();
            res
        }
    }
}

impl<'s, T: 's + ListNode> DoubleEndedIterator for IterPinMut<'s, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if ptr::eq(self.last, self.curr) {
            None
        } else {
            let last = unsafe { Pin::new_unchecked(&*self.last) };
            debug_assert_ne!(self.last, last.prev(), "loops forever");
            self.last = last.prev();
            // Safe since `self.last` is a `ListEntry` contained inside a `T`.
            let ptr = T::from_list_entry(self.last);
            Some(unsafe { Pin::new_unchecked(&mut *ptr) })
        }
    }
}

impl ListEntry {
    /// Returns an uninitialized `ListEntry`,
    ///
    /// # Safety
    ///
    /// All `ListEntry` types must be used only after initializing it with `ListEntry::init`.
    pub const unsafe fn new() -> Self {
        Self {
            prev: ptr::null_mut(),
            next: ptr::null_mut(),
            _marker: PhantomPinned,
        }
    }

    /// Gets a raw pointer from this `Pin` that points to the same referent.
    fn as_ptr(self: &mut Pin<&mut Self>) -> *mut Self {
        unsafe { self.as_mut().get_unchecked_mut() }
    }

    /// Initializes this `ListEntry` if it was not initialized.
    /// Otherwise, does nothing.
    ///
    /// # Note
    ///
    /// Do not call this method more than once.
    pub fn init(mut self: Pin<&mut Self>) {
        if self.next.is_null() {
            *self.as_mut().project().next = self.as_ptr();
            *self.as_mut().project().prev = self.as_ptr();
        }
    }

    /// Returns a raw pointer pointing to the previous `ListEntry`.
    ///
    /// # Note
    ///
    /// Do not use `ListNode::from_list_entry` on the returned pointer if `self` is the front node of a list.
    pub fn prev(self: Pin<&Self>) -> *mut Self {
        self.prev
    }

    /// Returns a raw pointer pointing to the next `ListEntry`.
    ///
    /// # Note
    ///
    /// Do not use `ListNode::from_list_entry` on the returned pointer if `self` is the back node of a list.
    pub fn next(self: Pin<&Self>) -> *mut Self {
        self.next
    }

    /// Returns `true` if this `ListEntry` is not linked to any other `ListEntry`.
    /// Otherwise, returns `false`.
    pub fn is_unlinked(self: Pin<&Self>) -> bool {
        ptr::eq(self.next, &*self)
    }

    /// Inserts `elt` at the back of this `ListEntry` after unlinking `elt`.
    fn push_back(mut self: Pin<&mut Self>, mut elt: Pin<&mut Self>) {
        if !elt.as_ref().is_unlinked() {
            elt.as_mut().remove();
        }

        *elt.as_mut().project().next = self.as_ptr();
        *elt.as_mut().project().prev = self.prev;
        unsafe {
            (*self.prev).next = elt.as_ptr();
        }
        *self.as_mut().project().prev = elt.as_ptr();
    }

    /// Inserts `elt` in front of this `ListEntry` after unlinking `elt`.
    fn push_front(mut self: Pin<&mut Self>, mut elt: Pin<&mut Self>) {
        if !elt.as_ref().is_unlinked() {
            elt.as_mut().remove();
        }

        *elt.as_mut().project().next = self.next;
        *elt.as_mut().project().prev = self.as_ptr();
        unsafe {
            (*self.next).prev = elt.as_ptr();
        }
        *self.as_mut().project().next = elt.as_ptr();
    }

    /// Unlinks this `ListEntry` from other `ListEntry`s.
    pub fn remove(mut self: Pin<&mut Self>) {
        unsafe {
            (*self.prev).next = self.next;
            (*self.next).prev = self.prev;
        }
        *self.as_mut().project().prev = self.as_ptr();
        *self.as_mut().project().next = self.as_ptr();
    }
}

#[pinned_drop]
impl PinnedDrop for ListEntry {
    fn drop(self: Pin<&mut Self>) {
        self.remove();
    }
}
