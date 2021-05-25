//! Array based arena.
use core::{ops::Deref, ptr::NonNull};

use array_macro::array;
use pin_project::pin_project;

use super::{Arena, ArenaObject, ArenaRef, Rc};
use crate::{
    lock::{Spinlock, SpinlockGuard},
    util::{
        branded::Branded,
        shared_rc_cell::{BrandedRcCell, BrandedRef, RcCell, SharedGuard},
    },
};

unsafe impl<T: Send, const CAPACITY: usize> Sync for ArrayArena<T, CAPACITY> {}

/// A homogeneous memory allocator equipped with reference counts.
#[pin_project]
pub struct ArrayArena<T, const CAPACITY: usize> {
    #[pin]
    entries: [RcCell<T>; CAPACITY],
    lock: Spinlock<()>,
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
            lock: Spinlock::new("ArrayArena", ()),
        }
    }
}

impl<'id, T, const CAPACITY: usize> ArenaRef<'id, &ArrayArena<T, CAPACITY>> {
    fn lock(&self) -> SharedGuard<'id, '_> {
        unsafe { SharedGuard::new_unchecked(self.0.brand(self.lock.lock())) }
    }

    fn entries(&self) -> EntryIter<'id, '_, T> {
        EntryIter(self.0.brand(self.entries.iter()))
    }
}

#[repr(transparent)]
struct EntryIter<'id, 's, T>(Branded<'id, core::slice::Iter<'s, RcCell<T>>>);

impl<'id: 's, 's, T> Iterator for EntryIter<'id, 's, T> {
    type Item = &'s BrandedRcCell<'id, T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .next()
            .map(|inner| unsafe { &*(inner as *const _ as *const _) })
    }
}

impl<T: 'static + ArenaObject + Unpin + Send, const CAPACITY: usize> Arena
    for ArrayArena<T, CAPACITY>
{
    type Data = T;
    type Guard<'s> = SpinlockGuard<'s, ()>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Rc<Self>> {
        ArenaRef::new(self, |arena: ArenaRef<'_, &ArrayArena<T, CAPACITY>>| {
            let mut guard = arena.lock();
            let mut empty = None;
            for cell in arena.entries() {
                let rc = cell.get_rc_mut(&mut guard);
                if *rc == 0 {
                    let _ = empty.get_or_insert(NonNull::from(cell));
                } else if let Some(data) = cell.get_data(&mut guard) {
                    if c(data) {
                        return Some(Rc::new(self, cell.make_ref(&mut guard).into_ref()));
                    }
                }
            }
            empty.map(|cell| {
                let cell = unsafe { cell.as_ref() };
                n(unsafe { cell.get_data_mut_unchecked(&mut guard) });
                Rc::new(self, cell.make_ref(&mut guard).into_ref())
            })
        })
    }

    fn alloc<F: FnOnce() -> Self::Data>(&self, f: F) -> Option<Rc<Self>> {
        ArenaRef::new(self, |arena: ArenaRef<'_, &ArrayArena<T, CAPACITY>>| {
            let mut guard = arena.lock();
            for cell in arena.entries() {
                if let Some(data) = cell.get_data_mut(&mut guard) {
                    *data = f();
                    return Some(Rc::new(self, cell.make_ref(&mut guard).into_ref()));
                }
            }
            None
        })
    }

    fn dup<'id>(self: ArenaRef<'id, &Self>, handle: &BrandedRef<'id, Self::Data>) -> Rc<Self> {
        let mut guard = self.lock();
        Rc::new(self.deref(), handle.clone(&mut guard).into_ref())
    }

    fn dealloc<'id>(self: ArenaRef<'id, &Self>, handle: BrandedRef<'id, Self::Data>) {
        let mut guard = self.lock();
        let handle = match handle.into_mut(&mut guard) {
            Ok(mut data) => {
                data.get_data_mut().finalize::<Self>(guard.inner_mut());
                data.into_ref(&mut guard)
            }
            Err(handle) => handle,
        };
        // To prevent `find_or_alloc` and `alloc` from mutating `handle` while finalizing `handle`,
        // `rc` should be decreased after the finalization.
        handle.free(&mut guard);
    }

    unsafe fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        guard.reacquire_after(f)
    }
}
