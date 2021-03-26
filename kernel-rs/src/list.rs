//! Doubly intrusive linked list with head node.
//! A [`List`] or [`ListNode`] must be first initialized before using its methods.
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
//! Instead, a [`List`] or [`ListNode`]'s methods never returns a reference to a node and always
//! returns a raw pointer instead. This is because a node could get mutated or dropped at any time, and hence,
//! the caller should make sure the node is not under mutation or already dropped when dereferencing the raw pointer.
// TODO: Also allow move.

use core::cell::Cell;
use core::marker::{PhantomData, PhantomPinned};
use core::mem;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;
use core::ptr;

use pin_project::{pin_project, pinned_drop};

/// A doubly linked list.
/// Can only contain `ListNode`s.
/// Use only after initialization.
///
/// # Safety
///
/// A `List` contains one or more `ListEntry`s.
/// * Exactly one of them is the `head`.
/// * All other of them are a `ListEntry` owned by a `ListNode`.
#[pin_project(PinnedDrop)]
pub struct List<T> {
    #[pin]
    head: ListEntry,
    _marker: PhantomData<T>,
}

/// An iterator over the `ListNode`s of a `List`.
pub struct Iter<'s, T> {
    last: &'s ListEntry,
    curr: &'s ListEntry,
    _marker: PhantomData<T>,
}

/// Intrusive linked list nodes that can be inserted into a `List`.
#[pin_project]
#[repr(C)]
pub struct ListNode<T> {
    #[pin]
    list_entry: ListEntry,
    data: T,
}

/// A low level primitive for doubly, intrusive linked lists and nodes.
///
/// # Safety
///
/// * All `ListEntry` types must be used only after initializing it with `ListEntry::init`.
/// After this, `ListEntry::{prev, next}` always refer to a valid, initialized `ListEntry`.
#[pin_project(PinnedDrop)]
struct ListEntry {
    prev: Cell<*const Self>,
    next: Cell<*const Self>,
    #[pin]
    _marker: PhantomPinned, //`ListEntry` is `!Unpin`.
}

impl<T> List<T> {
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

    /// Returns true if this `List` is empty.
    /// Otherwise, returns flase.
    pub fn is_empty(&self) -> bool {
        self.head.is_unlinked()
    }

    /// Provides a raw pointer to the back node, or `None` if the list is empty.
    pub fn back(&self) -> Option<*const ListNode<T>> {
        if self.is_empty() {
            None
        } else {
            Some(ListNode::from_list_entry(self.head.prev()))
        }
    }

    /// Provides a raw pointer to the front node, or `None` if the list is empty.
    pub fn front(&self) -> Option<*const ListNode<T>> {
        if self.is_empty() {
            None
        } else {
            Some(ListNode::from_list_entry(self.head.next()))
        }
    }

    /// Push `elt` at the back of the list after unlinking it.
    // TODO: Use PinFreeze<T>?
    pub fn push_back(&self, elt: &ListNode<T>) {
        self.head.push_back(&elt.list_entry);
    }

    /// Push `elt` at the front of the list after unlinking it.
    pub fn push_front(&self, elt: &ListNode<T>) {
        self.head.push_front(&elt.list_entry);
    }

    /// Removes the last node from the list and returns a raw pointer to it,
    /// or `None` if the list is empty.
    pub fn pop_back(&self) -> Option<*const ListNode<T>> {
        let ptr = self.head.prev();
        if ptr::eq(ptr, &self.head) {
            None
        } else {
            unsafe { (&*ptr).remove() };
            Some(ListNode::from_list_entry(ptr))
        }
    }

    /// Removes the last node from the list and returns a raw pointer to it,
    /// or `None` if the list is empty.
    pub fn pop_front(&self) -> Option<*const ListNode<T>> {
        let ptr = self.head.next();
        if ptr::eq(ptr, &self.head) {
            None
        } else {
            unsafe { (&*ptr).remove() };
            Some(ListNode::from_list_entry(ptr))
        }
    }

    /// Removes all nodes from the list.
    pub fn clear(&self) {
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
    pub unsafe fn iter_unchecked(&self) -> Iter<'_, T> {
        Iter {
            last: &self.head,
            curr: unsafe { &*self.head.next() },
            _marker: PhantomData,
        }
    }
}

#[pinned_drop]
impl<T> PinnedDrop for List<T> {
    fn drop(self: Pin<&mut Self>) {
        self.clear();
    }
}

impl<'s, T: 's> Iterator for Iter<'s, T> {
    type Item = &'s ListNode<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if ptr::eq(self.last, self.curr) {
            None
        } else {
            // Safe since `self.curr` is a `ListEntry` contained inside a `T`.
            let res = Some(unsafe { &*ListNode::from_list_entry(self.curr) });
            debug_assert_ne!(self.curr as *const _, self.curr.next(), "loops forever");
            self.curr = unsafe { &*self.curr.next() };
            res
        }
    }
}

impl<'s, T: 's> DoubleEndedIterator for Iter<'s, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if ptr::eq(self.last, self.curr) {
            None
        } else {
            debug_assert_ne!(self.last as *const _, self.last.prev(), "loops forever");
            self.last = unsafe { &*self.last.prev() };
            // Safe since `self.last` is a `ListEntry` contained inside a `T`.
            Some(unsafe { &*ListNode::from_list_entry(self.last) })
        }
    }
}

impl<T> ListNode<T> {
    // TODO(https://github.com/kaist-cp/rv6/issues/369)
    // A workarond for https://github.com/Gilnaa/memoffset/issues/49.
    // Assumes `list_entry` is located at the beginning of `ListNode`
    // and `data` is located at `mem::size_of::<ListEntry>()`.
    const DATA_OFFSET: usize = mem::size_of::<ListEntry>();
    const LIST_ENTRY_OFFSET: usize = 0;

    // const DATA_OFFSET: usize = offset_of!(ListNode<T>, data);
    // const LIST_ENTRY_OFFSET: usize = offset_of!(ListNode<T>, list_entry);

    /// Returns an uninitialized `ListNode`.
    ///
    /// # Safety
    ///
    /// All `ListNode` types must be used only after initializing it with `ListNode::init`.
    pub const unsafe fn new(data: T) -> Self {
        Self {
            data,
            list_entry: unsafe { ListEntry::new() },
        }
    }

    pub fn init(self: Pin<&mut Self>) {
        self.project().list_entry.init();
    }

    /// # Note
    ///
    /// Do not dereference the returned pointer if `self` is the head node.
    pub fn prev(&self) -> *const Self {
        Self::from_list_entry(self.list_entry.prev())
    }

    /// # Note
    ///
    /// Do not dereference the returned pointer if `self` is the head node.
    pub fn next(&self) -> *const Self {
        Self::from_list_entry(self.list_entry.next())
    }

    pub fn push_back(&self, elt: &Self) {
        self.list_entry.push_back(&elt.list_entry);
    }

    pub fn push_front(&self, elt: &Self) {
        self.list_entry.push_front(&elt.list_entry);
    }

    pub fn remove(&self) {
        self.list_entry.remove();
    }

    pub fn from_data(data: *const T) -> *const Self {
        (data as usize - Self::DATA_OFFSET) as *const Self
    }

    fn from_list_entry(list_entry: *const ListEntry) -> *const Self {
        (list_entry as usize - Self::LIST_ENTRY_OFFSET) as *const Self
    }
}

impl<T> Deref for ListNode<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T> DerefMut for ListNode<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
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
    fn push_back(&self, elt: &Self) {
        if !elt.is_unlinked() {
            elt.remove();
        }

        elt.next.set(self);
        elt.prev.set(self.prev());
        unsafe {
            (*elt.next()).prev.set(elt);
            (*elt.prev()).next.set(elt);
        }
    }

    /// Inserts `elt` in front of this `ListEntry` after unlinking `elt`.
    fn push_front(&self, elt: &Self) {
        if !elt.is_unlinked() {
            elt.remove();
        }

        elt.next.set(self.next());
        elt.prev.set(self);
        unsafe {
            (*elt.next()).prev.set(elt);
            (*elt.prev()).next.set(elt);
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
impl PinnedDrop for ListEntry {
    fn drop(self: Pin<&mut Self>) {
        self.remove();
    }
}
