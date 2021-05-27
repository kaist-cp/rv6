//! Array based arena.

use core::ptr::NonNull;

use array_macro::array;
use pin_project::pin_project;

use super::{Arena, ArenaObject, ArenaRef, Handle, HandleRef, Rc};
use crate::{
    lock::{Spinlock, SpinlockGuard},
    util::{rc_cell::RcCell, shared_mut::SharedMut},
};

/// A homogeneous memory allocator equipped with reference counts.
#[pin_project]
pub struct ArrayArena<T, const CAPACITY: usize> {
    #[pin]
    entries: [RcCell<T>; CAPACITY],
}

impl<T, const CAPACITY: usize> ArrayArena<T, CAPACITY> {
    /// Returns an `ArrayArena` of size `CAPACITY` that is filled with `D`'s const default value.
    /// Note that `D` must `impl const Default`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let arr_arena = ArrayArena::<D, 100>::new();
    /// ```
    // Note: We cannot use the generic `T` in the following function, since we need to only allow
    // types that `impl const Default`, not just `impl Default`.
    #[allow(clippy::new_ret_no_self)]
    pub const fn new<D: Default>() -> ArrayArena<D, CAPACITY> {
        ArrayArena {
            entries: array![_ => RcCell::new(Default::default()); CAPACITY],
        }
    }

    fn entries(this: SharedMut<'_, Self>) -> SharedMut<'_, [RcCell<T>; CAPACITY]> {
        // SAFETY: the pointer is valid, and it creates a unique `SharedMut`.
        unsafe { SharedMut::new_unchecked(&raw mut (*this.ptr().as_ptr()).entries) }
    }
}

impl<T: 'static + ArenaObject + Unpin + Send, const CAPACITY: usize> Arena
    for Spinlock<ArrayArena<T, CAPACITY>>
{
    type Data = T;
    type Guard<'s> = SpinlockGuard<'s, ArrayArena<T, CAPACITY>>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Rc<Self>> {
        ArenaRef::new(
            self,
            |arena: ArenaRef<'_, &Spinlock<ArrayArena<T, CAPACITY>>>| {
                let mut guard = arena.lock();
                let this = guard.get_shared_mut();

                let mut empty: Option<NonNull<RcCell<T>>> = None;
                for mut entry in ArrayArena::entries(this).iter() {
                    if !RcCell::is_borrowed(entry.as_shared_mut()) {
                        let _ = empty.get_or_insert(entry.ptr());
                        // Note: Do not use `break` here.
                        // We must first search through all entries, and then alloc at empty
                        // only if the entry we're finding for doesn't exist.
                    } else if let Some(entry) = RcCell::try_borrow(entry.as_shared_mut()) {
                        // The entry is not under finalization. Check its data.
                        if c(&entry) {
                            let handle = Handle(arena.0.brand(entry));
                            return Some(Rc::new(arena, handle));
                        }
                    }
                }

                empty.map(|ptr| {
                    // SAFETY: `ptr` is valid, and there's no `SharedMut`.
                    let mut entry = unsafe { SharedMut::new_unchecked(ptr.as_ptr()) };
                    n(RcCell::get_mut(entry.as_shared_mut()).unwrap());
                    let handle = Handle(arena.0.brand(RcCell::borrow(entry)));
                    Rc::new(arena, handle)
                })
            },
        )
    }

    fn alloc<F: FnOnce() -> Self::Data>(&self, f: F) -> Option<Rc<Self>> {
        ArenaRef::new(
            self,
            |arena: ArenaRef<'_, &Spinlock<ArrayArena<T, CAPACITY>>>| {
                let mut guard = arena.lock();
                let this = guard.get_shared_mut();

                for mut entry in ArrayArena::entries(this).iter() {
                    if let Some(data) = RcCell::get_mut(entry.as_shared_mut()) {
                        *data = f();
                        let handle = Handle(arena.0.brand(RcCell::borrow(entry)));
                        return Some(Rc::new(arena, handle));
                    }
                }
                None
            },
        )
    }

    fn dup<'id>(
        self: ArenaRef<'id, &Self>,
        handle: HandleRef<'id, '_, Self::Data>,
    ) -> Handle<'id, Self::Data> {
        Handle(self.0.brand(handle.clone()))
    }

    fn dealloc<'id>(self: ArenaRef<'id, &Self>, handle: Handle<'id, Self::Data>) {
        if let Ok(mut rm) = handle.0.into_inner().into_mut() {
            rm.finalize::<Self>();
        }
    }
}
