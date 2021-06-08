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
// TODO: Also allow move.

use core::cell::Cell;
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
pub struct Iter<'s, T: ListNode> {
    last: &'s ListEntry,
    curr: &'s ListEntry,
    _marker: PhantomData<T>,
}

pub struct IterStrongPinMut<'s, T> {
    last: *const ListEntry,
    curr: *const ListEntry,
    _marker: PhantomData<&'s mut T>,
}

/// A pinned mutable iterator over the elements of `List`.
pub struct IterPinMut<'s, T: ListNode> {
    last: *const ListEntry,
    curr: *const ListEntry,
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
    fn get_list_entry(self: Pin<&Self>) -> Pin<&ListEntry>;

    /// Returns a raw pointer which points to the struct that owns the given `list_entry`.
    /// You may want to use `offset_of!` to implement this.
    fn from_list_entry(list_entry: *const ListEntry) -> *const Self;
}

/// A low level primitive for doubly, intrusive linked lists and nodes.
///
/// # Safety
///
/// * All `ListEntry` types must be used only after initializing it with `ListEntry::init`.
/// After this, `ListEntry::{prev, next}` always refer to a valid, initialized `ListEntry`.
#[pin_project(PinnedDrop)]
pub struct ListEntry {
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
            _marker: PhantomData,
        }
    }

    /// Initializes this `ListEntry` if it was not initialized.
    /// Otherwise, does nothing.
    pub fn init(self: Pin<&mut Self>) {
        self.project().head.init();
    }

    fn head(self: Pin<&Self>) -> Pin<&ListEntry> {
        unsafe { Pin::new_unchecked(&self.get_ref().head) }
    }

    /// Returns true if this `List` is empty.
    /// Otherwise, returns flase.
    pub fn is_empty(self: Pin<&Self>) -> bool {
        self.head().is_unlinked()
    }

    /// Provides a raw pointer to the back node, or `None` if the list is empty.
    pub fn back(self: Pin<&Self>) -> Option<*const T> {
        if self.is_empty() {
            None
        } else {
            Some(T::from_list_entry(self.head().prev()))
        }
    }

    /// Provides a raw pointer to the front node, or `None` if the list is empty.
    pub fn front(self: Pin<&Self>) -> Option<*const T> {
        if self.is_empty() {
            None
        } else {
            Some(T::from_list_entry(self.head().next()))
        }
    }

    /// Push `elt` at the back of the list after unlinking it.
    pub fn push_back(self: Pin<&Self>, elt: Pin<&T>) {
        self.head().push_back(elt.get_list_entry());
    }

    /// Push `elt` at the front of the list after unlinking it.
    pub fn push_front(self: Pin<&Self>, elt: Pin<&T>) {
        self.head().push_front(elt.get_list_entry());
    }

    /// Removes the last node from the list and returns a raw pointer to it,
    /// or `None` if the list is empty.
    pub fn pop_back(self: Pin<&Self>) -> Option<*const T> {
        let ptr = self.head().prev();
        if ptr::eq(ptr, &self.head) {
            None
        } else {
            let ptr = unsafe { Pin::new_unchecked(&*ptr) };
            ptr.remove();
            Some(T::from_list_entry(ptr.get_ref()))
        }
    }

    /// Removes the last node from the list and returns a raw pointer to it,
    /// or `None` if the list is empty.
    pub fn pop_front(self: Pin<&Self>) -> Option<*const T> {
        let ptr = self.head().next();
        if ptr::eq(ptr, &self.head) {
            None
        } else {
            let ptr = unsafe { Pin::new_unchecked(&*ptr) };
            ptr.remove();
            Some(T::from_list_entry(ptr.get_ref()))
        }
    }

    /// Removes all nodes from the list.
    pub fn clear(self: Pin<&Self>) {
        while self.pop_front().is_some() {}
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
    /// #     fn get_list_entry(&self) -> &ListEntry {
    /// #         &self.list_entry
    /// #     }
    /// #
    /// #     fn from_list_entry(list_entry: *const ListEntry) -> *const Self {
    /// #         (list_entry as usize - offset_of!(Node, list_entry)) as *const Self
    /// #     }
    /// # }
    /// #
    /// # fn main() {
    ///     // Make and initialize a `List` and a `Node` that implements the `ListNode` trait.
    ///     let mut list = unsafe { List::new() };
    ///     let mut node = Some(unsafe { Node { data: 10, list_entry: ListEntry::new() }});
    ///     let list_pin = unsafe { Pin::new_unchecked(&mut list) };
    ///     let node_pin = unsafe { Pin::new_unchecked(node.as_mut().expect("")) };
    ///     list_pin.init();
    ///     node_pin.project().list_entry.init();
    ///
    ///     // Push the `ListNode` to the `List`.
    ///     list.push_front(node.as_ref().expect(""));
    ///
    ///     // Use an unsafe iterator.
    ///     for n in unsafe { list.iter_unchecked() } {
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
            last: self.head().get_ref(),
            curr: unsafe { &*self.head().next() },
            _marker: PhantomData,
        }
    }

    pub fn iter_shared_mut(this: StrongPinMut<'_, Self>) -> IterStrongPinMut<'_, T> {
        let last = unsafe { &(*this.ptr().as_ptr()).head };
        let curr = unsafe { &*Pin::new_unchecked(last).next() };
        IterStrongPinMut {
            last,
            curr,
            _marker: PhantomData,
        }
    }

    pub unsafe fn iter_pin_mut_unchecked(self: Pin<&mut Self>) -> IterPinMut<'_, T> {
        IterPinMut {
            last: &self.head,
            curr: unsafe { &*self.as_ref().head().next() },
            _marker: PhantomData,
        }
    }
}

#[pinned_drop]
impl<T: ListNode> PinnedDrop for List<T> {
    fn drop(self: Pin<&mut Self>) {
        self.as_ref().clear();
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
            let curr = unsafe { Pin::new_unchecked(self.curr) };
            debug_assert_ne!(self.curr as *const _, curr.next(), "loops forever");
            self.curr = unsafe { &*curr.next() };
            res
        }
    }
}

impl<'s, T: 's + ListNode> DoubleEndedIterator for Iter<'s, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if ptr::eq(self.last, self.curr) {
            None
        } else {
            let last = unsafe { Pin::new_unchecked(self.last) };
            debug_assert_ne!(self.last as *const _, last.prev(), "loops forever");
            self.last = unsafe { &*last.prev() };
            // Safe since `self.last` is a `ListEntry` contained inside a `T`.
            Some(unsafe { &*T::from_list_entry(self.last) })
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
            let ptr = T::from_list_entry(self.curr) as *mut T;
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
            let ptr = T::from_list_entry(self.last) as *mut T;
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
            let ptr = T::from_list_entry(self.curr) as *mut T;
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
            let ptr = T::from_list_entry(self.last) as *mut T;
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
            prev: Cell::new(ptr::null_mut()),
            next: Cell::new(ptr::null_mut()),
            _marker: PhantomPinned,
        }
    }

    /// Initializes this `ListEntry` if it was not initialized.
    /// Otherwise, does nothing.
    pub fn init(self: Pin<&mut Self>) {
        if self.as_ref().next().is_null() {
            self.next.set(self.as_ref().get_ref());
            self.prev.set(self.as_ref().get_ref());
        }
    }

    /// Returns a raw pointer pointing to the previous `ListEntry`.
    ///
    /// # Note
    ///
    /// Do not use `ListNode::from_list_entry` on the returned pointer if `self` is the front node of a list.
    pub fn prev(self: Pin<&Self>) -> *const Self {
        self.prev.get()
    }

    /// Returns a raw pointer pointing to the next `ListEntry`.
    ///
    /// # Note
    ///
    /// Do not use `ListNode::from_list_entry` on the returned pointer if `self` is the back node of a list.
    pub fn next(self: Pin<&Self>) -> *const Self {
        self.next.get()
    }

    /// Returns `true` if this `ListEntry` is not linked to any other `ListEntry`.
    /// Otherwise, returns `false`.
    pub fn is_unlinked(self: Pin<&Self>) -> bool {
        ptr::eq(self.next(), self.get_ref())
    }

    /// Inserts `elt` at the back of this `ListEntry` after unlinking `elt`.
    fn push_back(self: Pin<&Self>, elt: Pin<&Self>) {
        if !elt.is_unlinked() {
            elt.remove();
        }

        elt.next.set(self.get_ref());
        elt.prev.set(self.prev());
        unsafe {
            (*elt.next()).prev.set(elt.get_ref());
            (*elt.prev()).next.set(elt.get_ref());
        }
    }

    /// Inserts `elt` in front of this `ListEntry` after unlinking `elt`.
    fn push_front(self: Pin<&Self>, elt: Pin<&Self>) {
        if !elt.is_unlinked() {
            elt.remove();
        }

        elt.next.set(self.next());
        elt.prev.set(self.get_ref());
        unsafe {
            (*elt.next()).prev.set(elt.get_ref());
            (*elt.prev()).next.set(elt.get_ref());
        }
    }

    /// Unlinks this `ListEntry` from other `ListEntry`s.
    pub fn remove(self: Pin<&Self>) {
        unsafe {
            (*self.prev()).next.set(self.next());
            (*self.next()).prev.set(self.prev());
        }
        self.prev.set(self.get_ref());
        self.next.set(self.get_ref());
    }
}

#[pinned_drop]
impl PinnedDrop for ListEntry {
    fn drop(self: Pin<&mut Self>) {
        self.as_ref().remove();
    }
}
