use core::marker::PhantomData;
use core::pin::Pin;

use pin_project::pin_project;

use super::branded::Branded;
use super::list::ListEntry;

/// A doubly linked list that does not own the `Node`s and does not require `Node`s to outlive the `List`.
/// However, this list is logically the "borrow owner" of all of its inner `Node`s. That is,
/// * If you have a `ListRef`, you can immutably dereference all inner `Node`s.
/// * If you have a `ListMut`, you can mutably dereference all inner `Node`s.
#[pin_project]
pub struct List<T> {
    #[pin]
    head: ListEntry,
    _marker: PhantomData<T>,
}

pub struct ListRef<'id, 's, T>(Branded<'id, &'s List<T>>);
pub struct ListMut<'id, 's, T>(Branded<'id, Pin<&'s mut List<T>>>);

#[pin_project]
pub struct Node<T> {
    #[pin]
    list_entry: ListEntry,
    data: T, // UnsafeCell?
}

// TODO: Add `ListEntry`? (We don't actually need interior mutablility here)

pub struct NodeRef<'id, 's, T>(Branded<'id, &'s Node<T>>);
pub struct NodeMut<'id, 's, T>(Branded<'id, Pin<&'s mut Node<T>>>);

impl<T> List<T> {
    pub unsafe fn new() -> Self {
        Self {
            head: unsafe { ListEntry::new() },
            _marker: PhantomData,
        }
    }

    pub fn init(self: Pin<&mut Self>) {
        self.project().head.init();
    }

    pub fn get_ref<'s, F: for<'new_id> FnOnce(ListRef<'new_id, 's, T>) -> R, R>(
        &'s self,
        f: F,
    ) -> R {
        Branded::new(self, |handle| f(ListRef(handle)))
    }

    pub fn get_mut<'s, F: for<'new_id> FnOnce(ListMut<'new_id, 's, T>) -> R, R>(
        self: Pin<&'s mut Self>,
        f: F,
    ) -> R {
        Branded::new(self, |handle| f(ListMut(handle)))
    }
}

impl<'id, 's, T> ListRef<'id, 's, T> {
    pub fn is_empty(&self) -> bool {
        self.0.head.is_unlinked()
    }

    pub fn back(&self) -> Option<NodeRef<'id, 's, T>> {
        if self.is_empty() {
            None
        } else {
            let ptr = Node::from_list_entry(self.0.head.prev() as *mut _);
            Some(NodeRef(unsafe { self.0.brand(&*ptr) }))
        }
    }

    pub fn front(&self) -> Option<NodeRef<'id, 's, T>> {
        if self.is_empty() {
            None
        } else {
            let ptr = Node::from_list_entry(self.0.head.next() as *mut _);
            Some(NodeRef(unsafe { self.0.brand(&*ptr) }))
        }
    }
}

// TODO: Add iterator

impl<'id, 's, T> ListMut<'id, 's, T> {
    pub fn is_empty(&self) -> bool {
        self.0.head.is_unlinked()
    }

    pub fn back(&self) -> Option<NodeRef<'id, 's, T>> {
        if self.is_empty() {
            None
        } else {
            let ptr = Node::from_list_entry(self.0.head.prev() as *mut _);
            Some(NodeRef(unsafe { self.0.brand(&*ptr) }))
        }
    }

    pub fn front(&self) -> Option<NodeRef<'id, 's, T>> {
        if self.is_empty() {
            None
        } else {
            let ptr = Node::from_list_entry(self.0.head.next() as *mut _);
            Some(NodeRef(unsafe { self.0.brand(&*ptr) }))
        }
    }

    pub fn back_mut(&self) -> Option<NodeMut<'id, 's, T>> {
        if self.is_empty() {
            None
        } else {
            let ptr = Node::from_list_entry(self.0.head.prev() as *mut _);
            Some(NodeMut(unsafe {
                self.0.brand(Pin::new_unchecked(&mut *ptr))
            }))
        }
    }

    pub fn front_mut(&self) -> Option<NodeMut<'id, 's, T>> {
        if self.is_empty() {
            None
        } else {
            let ptr = Node::from_list_entry(self.0.head.next() as *mut _);
            Some(NodeMut(unsafe {
                self.0.brand(Pin::new_unchecked(&mut *ptr))
            }))
        }
    }

    pub fn push_back<'t>(&mut self, node: Pin<&'t mut Node<T>>) -> NodeMut<'id, 't, T> {
        self.0.head.push_back(&node.list_entry);
        unsafe { NodeMut(self.0.brand(node)) }
    }

    pub fn push_front<'t>(&mut self, node: Pin<&'t mut Node<T>>) -> NodeMut<'id, 't, T> {
        self.0.head.push_front(&node.list_entry);
        unsafe { NodeMut(self.0.brand(node)) }
    }

    pub fn pop_back(&mut self) {
        if let Some(node_mut) = self.back_mut() {
            node_mut.remove(self);
        }
    }

    pub fn pop_front(&mut self) {
        if let Some(node_mut) = self.front_mut() {
            node_mut.remove(self);
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

    pub unsafe fn get_ref_unchecked<'id, 's>(
        &'s self,
        list_ref: &ListRef<'id, '_, T>,
    ) -> NodeRef<'id, 's, T> {
        NodeRef(unsafe { list_ref.0.brand(self) })
    }

    pub unsafe fn get_mut_unchecked<'id, 's>(
        self: Pin<&'s mut Self>,
        list_mut: &mut ListMut<'id, '_, T>,
    ) -> NodeMut<'id, 's, T> {
        NodeMut(unsafe { list_mut.0.brand(self) })
    }

    fn from_list_entry(list_entry: *mut ListEntry) -> *mut Self {
        (list_entry as usize - Self::LIST_ENTRY_OFFSET) as *mut Self
    }
}

impl<'id, 's, T> NodeRef<'id, 's, T> {
    pub fn get(&self, _list_ref: &ListRef<'id, '_, T>) -> &T {
        &self.0.data
    }
}

impl<'id, 's, T> NodeMut<'id, 's, T> {
    pub fn get(&self, _list_mut: &ListMut<'id, '_, T>) -> &T {
        &self.0.data
    }

    pub fn get_mut(&mut self, _list_mut: &mut ListMut<'id, '_, T>) -> &mut T {
        &mut unsafe { self.0.as_mut().get_unchecked_mut() }.data
    }

    pub fn remove(self, _list_mut: &mut ListMut<'id, '_, T>) {
        self.0.list_entry.remove();
    }
}
