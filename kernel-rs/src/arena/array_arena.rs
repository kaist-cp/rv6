//! Array based arena.
use array_macro::array;
use pin_project::pin_project;

use super::{Arena, ArenaGuard, ArenaObject, ArenaRef, Entry, EntryRef, Rc};
use crate::{
    arena::Handle,
    lock::{Spinlock, SpinlockGuard},
    util::branded::Branded,
};

unsafe impl<T: Send, const CAPACITY: usize> Sync for ArrayArena<T, CAPACITY> {}

/// A homogeneous memory allocator equipped with reference counts.
#[pin_project]
pub struct ArrayArena<T, const CAPACITY: usize> {
    #[pin]
    entries: [Entry<T>; CAPACITY],
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
            entries: array![_ => Entry::new(Default::default()); CAPACITY],
            lock: Spinlock::new("ArrayArena", ()),
        }
    }
}

impl<'id, T, const CAPACITY: usize> ArenaRef<'id, &ArrayArena<T, CAPACITY>> {
    fn lock(&self) -> ArenaGuard<'id, '_> {
        ArenaGuard(self.0.brand(self.lock.lock()))
    }

    fn entries(&self) -> EntryIter<'id, '_, T> {
        EntryIter(self.0.brand(self.entries.iter()))
    }
}

#[repr(transparent)]
struct EntryIter<'id, 's, T>(Branded<'id, core::slice::Iter<'s, Entry<T>>>);

impl<'id, 's, T> Iterator for EntryIter<'id, 's, T> {
    type Item = EntryRef<'id, 's, T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|inner| EntryRef(self.0.brand(inner)))
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
            // If empty is Some(v), v.rc is 0.
            let mut empty: Option<Handle<T>> = None;
            for entry in arena.entries() {
                let rc = entry.get_mut_rc(&mut guard);
                if *rc == 0 {
                    let _ = empty.get_or_insert(entry.into_handle());
                } else if c(&entry) {
                    *rc += 1;
                    return Some(Rc::new(self, entry.into_handle()));
                }
            }
            empty.map(|handle| {
                // SAFETY: handle.rc is 0.
                n(unsafe { &mut *handle.data_raw() });
                let entry = EntryRef(arena.0.brand(&handle));
                *entry.get_mut_rc(&mut guard) += 1;
                Rc::new(self, handle)
            })
        })
    }

    fn alloc<F: FnOnce() -> Self::Data>(&self, f: F) -> Option<Rc<Self>> {
        ArenaRef::new(self, |arena: ArenaRef<'_, &ArrayArena<T, CAPACITY>>| {
            let mut guard = arena.lock();
            for entry in arena.entries() {
                let rc = entry.get_mut_rc(&mut guard);
                if *rc == 0 {
                    // SAFETY: handle.rc is 0.
                    unsafe { *entry.data_raw() = f() };
                    *rc += 1;
                    return Some(Rc::new(self, entry.into_handle()));
                }
            }
            None
        })
    }

    fn dup<'id>(self: ArenaRef<'id, &Self>, handle: &EntryRef<'id, '_, Self::Data>) {
        let mut guard = self.lock();
        let rc = handle.get_mut_rc(&mut guard);
        *rc += 1;
    }

    fn dealloc<'id>(self: ArenaRef<'id, &Self>, handle: EntryRef<'id, '_, Self::Data>) {
        let mut guard = self.lock();
        let rc = handle.get_mut_rc(&mut guard);
        if *rc == 1 {
            // SAFETY: handle.rc will become 0.
            unsafe { (*handle.data_raw()).finalize::<Self>(&mut guard.0) };
        }
        // To prevent `find_or_alloc` and `alloc` from mutating `handle` while finalizing `handle`,
        // `rc` should be decreased after the finalization.
        let rc = handle.get_mut_rc(&mut guard);
        *rc -= 1;
    }

    unsafe fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        guard.reacquire_after(f)
    }
}
