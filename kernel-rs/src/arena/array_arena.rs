//! Array based arena.

use core::{marker::PhantomPinned, ptr::NonNull};

use array_macro::array;
use kernel_aam::{
    static_arc::StaticArc,
    strong_pin::{StrongPin, StrongPinMut},
};
use pin_project::pin_project;

use super::{Arena, ArenaObject, ArenaRc};
use crate::lock::{SpinLock, SpinLockGuard};

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
    #[allow(clippy::new_ret_no_self)]
    pub const fn new<D: Default>(name: &'static str) -> ArrayArena<D, CAPACITY> {
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
                    return Some(ArenaRc::new(self, entry));
                }
            }
        }

        empty.map(|ptr| {
            let mut entry = unsafe { StrongPinMut::new_unchecked(ptr.as_ptr()) };
            n(unsafe { entry.as_mut().get_mut_unchecked() });
            ArenaRc::new(self, unsafe { entry.borrow_unchecked() })
        })
    }

    fn alloc<F: FnOnce() -> Self::Data>(self: StrongPin<'_, Self>, f: F) -> Option<ArenaRc<Self>> {
        let mut guard = self.inner().strong_pinned_lock();
        let this = guard.get_strong_pinned_mut();

        for mut entry in this.entries().iter_mut() {
            if let Some(data) = entry.as_mut().get_mut() {
                *data = f();
                return Some(ArenaRc::new(self, entry.borrow()));
            }
        }
        None
    }
}
