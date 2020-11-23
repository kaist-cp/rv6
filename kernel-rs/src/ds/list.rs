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
}

/// Retrieve object from ListEntry.
#[macro_export]
macro_rules! container_of {
    ($ptr:expr, $type:path, $field:ident) => {
        ($ptr as *const _ as usize - offset_of!($type, $field)) as *mut _
    };
}

#[inline]
pub unsafe fn list_init(e: *mut ListEntry) {
    (*e).next = e;
    (*e).prev = e;
}

#[inline]
pub unsafe fn list_append(l: *mut ListEntry, e: *mut ListEntry) {
    (*e).next = l;
    (*e).prev = (*l).prev;

    (*(*e).next).prev = e;
    (*(*e).prev).next = e;
}

#[inline]
pub unsafe fn list_prepend(l: *mut ListEntry, e: *mut ListEntry) {
    (*e).next = (*l).next;
    (*e).prev = l;

    (*(*e).next).prev = e;
    (*(*e).prev).next = e;
}

#[inline]
pub unsafe fn list_empty(l: *const ListEntry) -> bool {
    (*l).next as *const _ == l
}

#[inline]
pub unsafe fn list_remove(e: *mut ListEntry) {
    (*(*e).prev).next = (*e).next;
    (*(*e).next).prev = (*e).prev;
    list_init(e);
}

#[inline]
pub unsafe fn list_pop_front(l: &ListEntry) -> *mut ListEntry {
    let result = l.next;
    list_remove(result);
    result
}
