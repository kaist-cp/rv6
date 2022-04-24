//! Array based arena.

use core::{marker::PhantomPinned, ptr::NonNull};

use array_macro::array;
use pin_project::pin_project;

use super::{Arena, ArenaObject, ArenaRc};
use crate::{
    lock::{SpinLock, SpinLockGuard},
    util::{
        static_arc::StaticArc,
        strong_pin::{StrongPin, StrongPinMut},
    },
};

pub struct ArrayArena<T, const CAPACITY: usize> {
    inner: SpinLock<ArrayArenaInner<T, CAPACITY>>,
}

/// A homogeneous memory allocator equipped with reference counts.
#[pin_project]
pub struct ArrayArenaInner<T, const CAPACITY: usize> {
    #[pin]
    entries: [StaticArc<T>; CAPACITY],
    #[pin]
    _marker: PhantomPinned,
}

impl<T, const CAPACITY: usize> ArrayArena<T, CAPACITY> {
    /// Returns an `ArrayArena` of size `CAPACITY` that is filled with `D`'s const default value.
    /// Note that `D` must `impl const Default`. `name` is used when reporting synchronization errors.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let arr_arena = ArrayArena::<D, 100>::new("arr_arena");
    /// ```
    #[allow(clippy::new_ret_no_self)]
    pub const fn new<D: ~const Default>(name: &'static str) -> ArrayArena<D, CAPACITY> {
        let inner: ArrayArenaInner<D, CAPACITY> = ArrayArenaInner {
            entries: array![_ => StaticArc::new(Default::default()); CAPACITY],
            _marker: PhantomPinned,
        };
        ArrayArena {
            inner: SpinLock::new(name, inner),
        }
    }

    #[allow(clippy::needless_lifetimes)]
    fn inner<'s>(
        self: StrongPin<'s, Self>,
    ) -> StrongPin<'s, SpinLock<ArrayArenaInner<T, CAPACITY>>> {
        unsafe { StrongPin::new_unchecked(&(*self.ptr()).inner) }
    }
}

impl<T, const CAPACITY: usize> ArrayArenaInner<T, CAPACITY> {
    #[allow(clippy::needless_lifetimes)]
    fn entries<'s>(self: StrongPinMut<'s, Self>) -> StrongPinMut<'s, [StaticArc<T>; CAPACITY]> {
        // SAFETY: the pointer is valid, and it creates a unique `StrongPinMut`.
        unsafe { StrongPinMut::new_unchecked(&raw mut (*self.ptr().as_ptr()).entries) }
    }
}

impl<T: 'static + ArenaObject + Unpin + Send, const CAPACITY: usize> Arena
    for ArrayArena<T, CAPACITY>
{
    type Data = T;
    type Guard<'s> = SpinLockGuard<'s, ArrayArenaInner<T, CAPACITY>>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        self: StrongPin<'_, Self>,
        c: C,
        n: N,
    ) -> Option<ArenaRc<Self>> {
        let mut guard = self.inner().strong_pinned_lock();
        let this = guard.get_strong_pinned_mut();

        let mut empty: Option<NonNull<StaticArc<T>>> = None;
        for mut entry in this.entries().iter_mut() {
            if !entry.as_mut().is_borrowed() {
                let _ = empty.get_or_insert(entry.ptr());
                // Note: Do not use `break` here.
                // We must first search through all entries, and then alloc at empty
                // only if the entry we're finding for doesn't exist.
            } else if let Some(entry) = entry.try_borrow() {
                if c(&entry) {
                    return Some(unsafe { ArenaRc::new(self, entry) });
                }
            }
        }

        empty.map(|ptr| {
            unsafe {
                let mut entry = StrongPinMut::new_unchecked(ptr.as_ptr());
                n(entry.as_mut().get_mut_unchecked());
                ArenaRc::new(self, entry.borrow_unchecked())
            }
        })
    }

    fn alloc<F: FnOnce() -> Self::Data>(self: StrongPin<'_, Self>, f: F) -> Option<ArenaRc<Self>> {
        let mut guard = self.inner().strong_pinned_lock();
        let this = guard.get_strong_pinned_mut();

        for mut entry in this.entries().iter_mut() {
            if let Some(data) = entry.as_mut().get_mut() {
                *data = f();
                return Some(unsafe { ArenaRc::new(self, entry.borrow()) });
            }
        }
        None
    }
}
