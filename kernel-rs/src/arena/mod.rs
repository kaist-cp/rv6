//! The arena module.
//!
//! Includes the `Arena` trait, which represents a type that can be used as an arena.
//! For types that `impl Arena`, you can allocate a thread safe `Rc` (reference counted pointer) from it.
//!
//! This module also includes pre-built arenas, such as `ArrayArena`(array based arena) or `MruArena`(list based arena).

use core::mem::ManuallyDrop;
use core::ops::Deref;

use crate::util::{
    branded::Branded,
    shared_rc_cell::{BrandedRef, Ref},
};

mod array_arena;
mod mru_arena;

pub use array_arena::ArrayArena;
pub use mru_arena::MruArena;

/// A homogeneous memory allocator. Provides `Rc<Arena>` to the outside.
pub trait Arena: Sized + Sync {
    /// The value type of the allocator.
    type Data: ArenaObject;
    /// The guard type for arena.
    type Guard<'s>;

    /// Looks for an `Rc` that already contains the data, and clone it if exists. Otherwise, we allocate a new `Rc`.
    /// * Uses `c` to check if the data is the one we are looking for.
    /// * Uses `n` to initialize a new `Rc`.
    ///
    /// If an empty entry does not exist, returns `None`.
    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Rc<Self>>;

    /// Allocates an `Rc` using the first empty entry.
    /// * Uses `f` to initialze a new `Rc`.
    ///
    /// Otherwise, returns `None`.
    fn alloc<F: FnOnce() -> Self::Data>(&self, f: F) -> Option<Rc<Self>>;

    /// Duplicates a given handle, increasing the reference count.
    ///
    /// # Note
    ///
    /// This method is automatically used by the `Rc`.
    /// Usually, you don't need to manually call this method.
    fn dup<'id>(self: ArenaRef<'id, &Self>, handle: &BrandedRef<'id, Self::Data>) -> Rc<Self>;

    /// Deallocate a given handle, decreasing the reference count
    /// Finalizes the referred object if there are no more handles.
    ///
    /// # Note
    ///
    /// This method is automatically used by the `Rc`.
    /// Usually, you don't need to manually call this method.
    fn dealloc<'id>(self: ArenaRef<'id, &Self>, handle: BrandedRef<'id, Self::Data>);

    /// Temporarily releases the lock while calling `f`, and re-acquires the lock after `f` returned.
    ///
    /// # Safety
    ///
    /// The caller must be careful when calling this inside `ArenaObject::finalize`.
    /// If you use this while finalizing an `ArenaObject`, the `Arena`'s lock will be temporarily released,
    /// and hence, another thread may use `Arena::find_or_alloc` to obtain an `Rc` referring to the `ArenaObject`
    /// we are **currently finalizing**. Therefore, in this case, make sure no thread tries to `find_or_alloc`
    /// for an `ArenaObject` that may be under finalization.
    unsafe fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R;
}

/// A branded reference to an arena.
///
/// # Safety
///
/// The `'id` is always different between different `Arena` instances.
#[derive(Clone, Copy)]
pub struct ArenaRef<'id, P: Deref>(Branded<'id, P>);

impl<'id, A: Arena> ArenaRef<'id, &A> {
    /// Creates a new `ArenaRef` that has a unique, invariant `'id` tag.
    /// The `ArenaRef` can be used only inside the given closure.
    #[allow(clippy::new_ret_no_self)]
    pub fn new<F: for<'new_id> FnOnce(ArenaRef<'new_id, &A>) -> R, R>(arena: &A, f: F) -> R {
        Branded::new(arena, |a| f(ArenaRef(a)))
    }
}

impl<'id, P: Deref> Deref for ArenaRef<'id, P> {
    type Target = P::Target;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A thread-safe reference counted pointer, allocated from `A: Arena`.
/// The data type is same as `A::Data`.
///
/// # Safety
///
/// `inner` is allocated from `arena`.
/// We can safely dereference `arena` until `inner` gets dropped,
/// because we panic if the arena drops earlier than `inner`.
pub struct Rc<A: Arena> {
    arena: *const A,
    inner: ManuallyDrop<Ref<A::Data>>,
}

// `Rc` is `Send` because it does not impl `DerefMut`,
// and when we access the inner `Arena`, we do it after acquiring `Arena`'s lock.
// Also, `Rc` does not point to thread-local data.
unsafe impl<T: Sync, A: Arena<Data = T>> Send for Rc<A> {}

impl<T, A: Arena<Data = T>> Rc<A> {
    /// Creates a new `Rc`, allocated from the arena.
    pub fn new(arena: *const A, inner: Ref<T>) -> Self {
        Self {
            arena,
            inner: ManuallyDrop::new(inner),
        }
    }

    fn arena(&self) -> &A {
        // SAFETY: Safe because of `Rc`'s invariant.
        unsafe { &*self.arena }
    }
}

impl<T, A: Arena<Data = T>> Deref for Rc<A> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<A: Arena> Drop for Rc<A> {
    fn drop(&mut self) {
        let r = unsafe { ManuallyDrop::take(&mut self.inner) };
        ArenaRef::new(self.arena(), |arena: ArenaRef<'_, &A>| {
            arena.dealloc(unsafe { BrandedRef::new_unchecked(arena.0.brand(r)) })
        });
    }
}

impl<A: Arena> Clone for Rc<A> {
    fn clone(&self) -> Self {
        ArenaRef::new(self.arena(), |arena: ArenaRef<'_, &A>| {
            let r: &Ref<_> = self.inner.deref();
            let handle: &BrandedRef<'_, _> = unsafe { &*(r as *const _ as *const _) };
            arena.dup(handle)
        })
    }
}

pub trait ArenaObject {
    /// Finalizes the `ArenaObject`.
    /// This function is automatically called when the last `Rc` refereing to this `ArenaObject` gets dropped.
    fn finalize<'s, A: Arena>(&'s mut self, guard: &'s mut A::Guard<'_>);
}
