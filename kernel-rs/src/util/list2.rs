//! A doubly linked list that does not owns its nodes.
//!
//! In Rust, a list usually owns its nodes. This is the easiest way to guarantee safety,
//! since we can access the elements only through the list's API in this way.
//!
//! However, often, we need a list that does not owns its nodes.
//! For example, the nodes may need to be scattered all around instead of being together in a single array.
//! Or, the nodes may need to be stored not only on the heap, but also on the stack or global data.
//! To implement such lists in a safe way, we usually needed to add extra restrictions on functionality,
//! such as that the node can never be removed from the list once it gets inserted,
//! or that the node must not drop before the list drops (even after the node was already popped from the list), etc.
//! Otherwise, we needed to add a lot of runtime cost.
//!
//! In this module's [`List`], we use a bit different way. We used the following invariants.
//! * `NodeRef`/`NodeMut` exists only while the `Node` is inside a `List`.
//! * Only one `NodeMut` exists for each `List`.
//! * To obtain a `NodeRef`, we need to immutably borrow both the node *and the list*.
//! * To obtain a `NodeMut`, we need to mutably borrow both the node *and the list*.
//! * If a `Node` drops while its still inside a `List`, we panic. (This is the only runtime cost we have.)
// TODO: Add documentation

use core::marker::PhantomData;
use core::pin::Pin;

use pin_project::{pin_project, pinned_drop};

use super::list::ListEntry;

/// A doubly linked list that does not own the `Node`s.
/// * A `Node` can be stored anywhere. (e.g. On the stack, heap, global data, etc.)
/// * A `Node` can drop at any time if it is not inside a `List`.
/// (i.e. A `Node` does not need to statically outlive the `List`, and conversely, the `List` does not need to statically outlive the `Node`s.)
/// * However, a `Node` must be pinned, and it must not drop while inside a `List`.
///
/// The list is logically the "borrow owner" of all of its inner `Node`s. That is,
/// * If you have a `ListRef`, you can immutably dereference any inner `Node`s.
/// * If you have a `ListMut`, you can mutably dereference any inner `Node`s.
#[pin_project]
pub struct List<T> {
    #[pin]
    head: ListEntry,
    _marker: PhantomData<T>,
}

/// An immutable reference to a `List`.
/// Grants immutable access to the `List` and any of its `Node`s.
// SAFETY: No `NodeMut` exists.
pub struct ListRef<'s, T>(&'s List<T>);

/// A mutable reference to a `List`.
/// Grants unique mutable access to the `List` and any of its `Node`s.
// SAFETY: No `NodeRef` exists.
//         Only one `NodeMut` exists for each `ListMut`.
pub struct ListMut<'s, T>(Pin<&'s mut List<T>>);

/// A node that can be inserted into a `List`.
/// * To actually read the inner data, you need a `NodeRef` (which needs a `ListRef`).
/// * To actually mutate the inner data or insert/remote this node into/from a `List`, you need a `NodeMut` (which needs a `ListMut`).
/// * Before dropping this `Node`, you must first remove this node from the `List`.
// SAFETY: A `Node` does not drop while a `NodeRef`/`NodeMut` exists. (Uses a single `assert!` to check this)
#[pin_project(PinnedDrop)]
pub struct Node<T> {
    #[pin]
    list_entry: ListEntry,
    data: T,
}

// TODO: Add `ListEntry`? (We don't actually need interior mutablility here)

/// An immutable reference to a `Node` that is inserted inside a `List`.
// SAFETY: The `Node` is already inserted inside a `List`.
//         `NodeRef` immutably borrows both the 1) `ListRef` and the 2) `Node` for `'s`.
pub struct NodeRef<'s, T>(&'s Node<T>);

/// A mutable reference to a `Node` that is inserted inside a `List`.
// SAFETY: The `Node` is already inserted inside a `List`.
//         `NodeMut` mutably borrows both the 1) `ListMut` and the 2) `Node` for `'s`.
pub struct NodeMut<'s, T>(Pin<&'s mut Node<T>>);

impl<T> List<T> {
    /// Returns a new `List`.
    /// Use `List::get_ref` or `List::get_mut` to do something with the `List`.
    pub unsafe fn new() -> Self {
        Self {
            head: unsafe { ListEntry::new() },
            _marker: PhantomData,
        }
    }

    /// Initializes the `List`.
    pub fn init(self: Pin<&mut Self>) {
        self.project().head.init();
    }

    /// Returns a `ListRef` of this `List`.
    pub fn get_ref(&self) -> ListRef<'_, T> {
        ListRef(self)
    }

    /// Returns a `ListMut` of this `List`.
    pub fn get_mut(self: Pin<&mut Self>) -> ListMut<'_, T> {
        ListMut(self)
    }
}

impl<'s, T> ListRef<'s, T> {
    /// Returns `true` if the `List` is empty.
    pub fn is_empty(&self) -> bool {
        self.0.head.is_unlinked()
    }

    /// Provides a `NodeRef` to the back element, or `None` if the list is empty.
    pub fn back(&self) -> Option<NodeRef<'_, T>> {
        if self.is_empty() {
            None
        } else {
            let ptr = Node::from_list_entry(self.0.head.prev() as *mut _);
            Some(NodeRef(unsafe { &*ptr }))
        }
    }

    /// Provides a `NodeRef` to the front element, or `None` if the list is empty.
    pub fn front(&self) -> Option<NodeRef<'_, T>> {
        if self.is_empty() {
            None
        } else {
            let ptr = Node::from_list_entry(self.0.head.next() as *mut _);
            Some(NodeRef(unsafe { &*ptr }))
        }
    }
}

// TODO: Add iterator

impl<'s, T> ListMut<'s, T> {
    pub fn is_empty(&self) -> bool {
        self.0.head.is_unlinked()
    }

    pub fn back(&self) -> Option<NodeRef<'_, T>> {
        if self.is_empty() {
            None
        } else {
            let ptr = Node::from_list_entry(self.0.head.prev() as *mut _);
            Some(NodeRef(unsafe { &*ptr }))
        }
    }

    pub fn front(&self) -> Option<NodeRef<'_, T>> {
        if self.is_empty() {
            None
        } else {
            let ptr = Node::from_list_entry(self.0.head.next() as *mut _);
            Some(NodeRef(unsafe { &*ptr }))
        }
    }

    // SAFETY: `NodeMut` borrows the `List`, but does not actually borrow the `Node`.
    //         However, this is safe since only one `NodeMut` exists for each `List` anyway.
    pub fn back_mut(&mut self) -> Option<NodeMut<'_, T>> {
        if self.is_empty() {
            None
        } else {
            let ptr = Node::from_list_entry(self.0.head.prev() as *mut _);
            Some(NodeMut(unsafe { Pin::new_unchecked(&mut *ptr) }))
        }
    }

    // SAFETY: `NodeMut` borrows the `List`, but does not actually borrow the `Node`.
    //         However, this is safe since only one `NodeMut` exists for each `List` anyway.
    pub fn front_mut(&mut self) -> Option<NodeMut<'_, T>> {
        if self.is_empty() {
            None
        } else {
            let ptr = Node::from_list_entry(self.0.head.next() as *mut _);
            Some(NodeMut(unsafe { Pin::new_unchecked(&mut *ptr) }))
        }
    }

    // SAFETY: `NodeMut` borrows the `List`, but does not actually borrow the `Node`.
    //         However, this is safe since only one `NodeMut` exists for each `List` anyway.
    pub fn push_back<'t>(&'t mut self, node: Pin<&'t mut Node<T>>) -> NodeMut<'t, T> {
        self.0.head.push_back(&node.list_entry);
        NodeMut(node)
    }

    // SAFETY: `NodeMut` borrows the `List`, but does not actually borrow the `Node`.
    //         However, this is safe since only one `NodeMut` exists for each `List` anyway.
    pub fn push_front<'t>(&'t mut self, node: Pin<&'t mut Node<T>>) -> NodeMut<'t, T> {
        self.0.head.push_front(&node.list_entry);
        NodeMut(node)
    }

    pub fn pop_back(&mut self) {
        if let Some(node_mut) = self.back_mut() {
            node_mut.remove();
        }
    }

    pub fn pop_front(&mut self) {
        if let Some(node_mut) = self.front_mut() {
            node_mut.remove();
        }
    }
}

// TODO: Add iterator

impl<T> Node<T> {
    const LIST_ENTRY_OFFSET: usize = 0;

    // TODO: use `offset_of!` instead.

    pub unsafe fn new(data: T) -> Self {
        Self {
            list_entry: unsafe { ListEntry::new() },
            data,
        }
    }

    pub fn init(self: Pin<&mut Self>) {
        self.project().list_entry.init();
    }

    /// # Safety
    ///
    /// The `Node` must already be inserted inside the list.
    pub unsafe fn get_ref_unchecked<'s>(&'s self, _list_ref: &'s ListRef<'_, T>) -> NodeRef<'s, T> {
        NodeRef(self)
    }

    /// # Safety
    ///
    /// The `Node` must already be inserted inside the list.
    pub unsafe fn get_mut_unchecked<'s>(
        self: Pin<&'s mut Self>,
        _list_mut: &'s mut ListMut<'_, T>,
    ) -> NodeMut<'s, T> {
        NodeMut(self)
    }

    fn from_list_entry(list_entry: *mut ListEntry) -> *mut Self {
        (list_entry as usize - Self::LIST_ENTRY_OFFSET) as *mut Self
    }
}

#[pinned_drop]
impl<T> PinnedDrop for Node<T> {
    fn drop(self: Pin<&mut Self>) {
        // A `Node` should not drop while its inside a `List`. It should always be removed first.
        // Note that we can't do this implicitly, since the `drop` function only takes 1 argument.
        assert!(
            self.project().list_entry.is_unlinked(),
            "Node dropped while its inside a list"
        );
    }
}

impl<'s, T> NodeRef<'s, T> {
    pub fn get(&self) -> &T {
        &self.0.data
    }
}

impl<'s, T> NodeMut<'s, T> {
    pub fn get(&self) -> &T {
        &self.0.data
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.0.as_mut().project().data
    }

    pub fn remove(self) {
        self.0.list_entry.remove();
    }
}
