//! Array based arena.

use core::convert::TryFrom;
use core::pin::Pin;

use array_macro::array;
use pin_project::pin_project;

use super::{Arena, ArenaObject, ArenaRef, Handle, HandleRef, Rc};
use crate::{
    lock::{Spinlock, SpinlockGuard},
    util::pinned_array::IterPinMut,
    util::rc_cell::{RcCell, RefMut},
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
        ArenaRef::new(self, |arena| {
            let mut guard = arena.pinned_lock_unchecked();
            let this = guard.get_pin_mut().project();

            let mut empty: Option<*mut RcCell<T>> = None;
            for entry in IterPinMut::from(this.entries) {
                if !entry.is_borrowed() {
                    if empty.is_none() {
                        empty = Some(entry.as_ref().get_ref() as *const _ as *mut _)
                    }
                    // Note: Do not use `break` here.
                    // We must first search through all entries, and then alloc at empty
                    // only if the entry we're finding for doesn't exist.
                } else if let Some(r) = entry.try_borrow() {
                    // The entry is not under finalization. Check its data.
                    if c(&r) {
                        return Some(Rc::new(arena, Handle(arena.0.brand(r))));
                    }
                }
            }

            empty.map(|cell_raw| {
                // SAFETY: `cell` is not referenced or borrowed. Also, it is already pinned.
                let mut cell = unsafe { Pin::new_unchecked(&mut *cell_raw) };
                n(cell.as_mut().get_pin_mut().unwrap().get_mut());
                let handle = Handle(arena.0.brand(cell.borrow()));
                Rc::new(arena, handle)
            })
        })
    }

    fn alloc<F: FnOnce() -> Self::Data>(&self, f: F) -> Option<Rc<Self>> {
        ArenaRef::new(self, |arena| {
            let mut guard = arena.pinned_lock_unchecked();
            let this = guard.get_pin_mut().project();

            for mut entry in IterPinMut::from(this.entries) {
                if !entry.is_borrowed() {
                    *(entry.as_mut().get_pin_mut().unwrap().get_mut()) = f();
                    let handle = Handle(arena.0.brand(entry.borrow()));
                    return Some(Rc::new(arena, handle));
                }
            }
            None
        })
    }

    fn dup<'id>(
        self: ArenaRef<'id, &Self>,
        handle: HandleRef<'id, '_, Self::Data>,
    ) -> Handle<'id, Self::Data> {
        let mut _this = self.pinned_lock_unchecked();
        Handle(self.0.brand(handle.0.into_inner().clone()))
    }

    fn dealloc<'id>(self: ArenaRef<'id, &Self>, handle: Handle<'id, Self::Data>) {
        let mut this = self.pinned_lock_unchecked();

        if let Ok(mut rm) = RefMut::<T>::try_from(handle.0.into_inner()) {
            rm.finalize::<Self>(&mut this);
        }
    }

    unsafe fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        guard.reacquire_after(f)
    }
}
