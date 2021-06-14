//! Array based arena.

use core::{marker::PhantomPinned, ptr::NonNull};

use array_macro::array;
use pin_project::pin_project;

use super::{Arena, ArenaObject, ArenaRc, ArenaRef, Handle};
use crate::{
    lock::{SpinLock, SpinLockGuard},
    util::{
        static_arc::StaticArc,
        strong_pin::{StrongPin, StrongPinMut},
    },
};

/// A homogeneous memory allocator equipped with reference counts.
#[pin_project]
pub struct ArrayArena<T, const CAPACITY: usize> {
    #[pin]
    entries: [StaticArc<T>; CAPACITY],
    #[pin]
    _marker: PhantomPinned,
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
            entries: array![_ => StaticArc::new(Default::default()); CAPACITY],
            _marker: PhantomPinned,
        }
    }

    #[allow(clippy::needless_lifetimes)]
    fn entries<'s>(self: StrongPinMut<'s, Self>) -> StrongPinMut<'s, [StaticArc<T>; CAPACITY]> {
        // SAFETY: the pointer is valid, and it creates a unique `StrongPinMut`.
        unsafe { StrongPinMut::new_unchecked(&raw mut (*self.ptr().as_ptr()).entries) }
    }
}

impl<T: 'static + ArenaObject + Unpin + Send, const CAPACITY: usize> Arena
    for SpinLock<ArrayArena<T, CAPACITY>>
{
    type Data = T;
    type Guard<'s> = SpinLockGuard<'s, ArrayArena<T, CAPACITY>>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        self: StrongPin<'_, Self>,
        c: C,
        n: N,
    ) -> Option<ArenaRc<Self>> {
        ArenaRef::new(
            self,
            |arena: ArenaRef<'_, '_, SpinLock<ArrayArena<T, CAPACITY>>>| {
                let mut guard = arena.strong_pinned_lock();
                let this = guard.get_strong_pinned_mut();

                let mut empty: Option<NonNull<StaticArc<T>>> = None;
                for mut entry in this.entries().iter_mut() {
                    if !entry.as_mut().is_borrowed() {
                        let _ = empty.get_or_insert(entry.ptr());
                        // Note: Do not use `break` here.
                        // We must first search through all entries, and then alloc at empty
                        // only if the entry we're finding for doesn't exist.
                    } else if let Some(entry) = entry.as_mut().try_borrow() {
                        // The entry is not under finalization. Check its data.
                        if c(&entry) {
                            let handle = Handle(arena.0.brand(entry));
                            return Some(ArenaRc::new(arena, handle));
                        }
                    }
                }

                empty.map(|ptr| {
                    // SAFETY: `ptr` is valid, and there's no `StrongPinMut`.
                    let mut entry = unsafe { StrongPinMut::new_unchecked(ptr.as_ptr()) };
                    n(entry.as_mut().get_mut().unwrap());
                    let handle = Handle(arena.0.brand(entry.borrow()));
                    ArenaRc::new(arena, handle)
                })
            },
        )
    }

    fn alloc<F: FnOnce() -> Self::Data>(self: StrongPin<'_, Self>, f: F) -> Option<ArenaRc<Self>> {
        ArenaRef::new(
            self,
            |arena: ArenaRef<'_, '_, SpinLock<ArrayArena<T, CAPACITY>>>| {
                let mut guard = arena.strong_pinned_lock();
                let this = guard.get_strong_pinned_mut();

                for mut entry in this.entries().iter_mut() {
                    if let Some(data) = entry.as_mut().get_mut() {
                        *data = f();
                        let handle = Handle(arena.0.brand(entry.borrow()));
                        return Some(ArenaRc::new(arena, handle));
                    }
                }
                None
            },
        )
    }
}
