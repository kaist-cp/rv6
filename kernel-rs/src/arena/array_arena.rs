//! Array based arena.

use core::{marker::PhantomPinned, pin::Pin, ptr::NonNull};

use array_macro::array;
use pin_project::pin_project;

use super::{Arena, ArenaObject, ArenaRc, ArenaRef, Handle};
use crate::{
    lock::{Spinlock, SpinlockGuard},
    util::{shared_mut::SharedMut, static_arc::StaticArc},
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

    fn entries(this: SharedMut<'_, Self>) -> SharedMut<'_, [StaticArc<T>; CAPACITY]> {
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
        self: Pin<&Self>,
        c: C,
        n: N,
    ) -> Option<ArenaRc<Self>> {
        ArenaRef::new(
            self,
            |arena: ArenaRef<'_, '_, Spinlock<ArrayArena<T, CAPACITY>>>| {
                let mut guard = arena.pinned_lock();
                let this = guard.get_shared_mut();

                let mut empty: Option<NonNull<StaticArc<T>>> = None;
                for mut entry in ArrayArena::entries(this).iter() {
                    if !StaticArc::is_borrowed(entry.as_shared_mut()) {
                        let _ = empty.get_or_insert(entry.ptr());
                        // Note: Do not use `break` here.
                        // We must first search through all entries, and then alloc at empty
                        // only if the entry we're finding for doesn't exist.
                    } else if let Some(entry) = StaticArc::try_borrow(entry.as_shared_mut()) {
                        // The entry is not under finalization. Check its data.
                        if c(&entry) {
                            let handle = Handle(arena.0.brand(entry));
                            return Some(ArenaRc::new(arena, handle));
                        }
                    }
                }

                empty.map(|ptr| {
                    // SAFETY: `ptr` is valid, and there's no `SharedMut`.
                    let mut entry = unsafe { SharedMut::new_unchecked(ptr.as_ptr()) };
                    n(StaticArc::get_mut(entry.as_shared_mut()).unwrap());
                    let handle = Handle(arena.0.brand(StaticArc::borrow(entry)));
                    ArenaRc::new(arena, handle)
                })
            },
        )
    }

    fn alloc<F: FnOnce() -> Self::Data>(self: Pin<&Self>, f: F) -> Option<ArenaRc<Self>> {
        ArenaRef::new(
            self,
            |arena: ArenaRef<'_, '_, Spinlock<ArrayArena<T, CAPACITY>>>| {
                let mut guard = arena.pinned_lock();
                let this = guard.get_shared_mut();

                for mut entry in ArrayArena::entries(this).iter() {
                    if let Some(data) = StaticArc::get_mut(entry.as_shared_mut()) {
                        *data = f();
                        let handle = Handle(arena.0.brand(StaticArc::borrow(entry)));
                        return Some(ArenaRc::new(arena, handle));
                    }
                }
                None
            },
        )
    }
}
