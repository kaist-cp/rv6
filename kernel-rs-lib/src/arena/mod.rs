//! The arena module.
//!
//! Includes the `Arena` trait, which represents a type that can be used as an arena.
//! For types that `impl Arena`, you can allocate a thread safe `Rc` (reference counted pointer) from it.
//!
//! This module also includes pre-built arenas, such as `ArrayArena`(array based arena) or `MruArena`(list based arena).

use core::mem::ManuallyDrop;
use core::ops::Deref;

use crate::{static_arc::Ref, strong_pin::StrongPin};

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
        self: StrongPin<'_, Self>,
        c: C,
        n: N,
    ) -> Option<ArenaRc<Self>>;

    /// Allocates an `Rc` using the first empty entry.
    /// * Uses `f` to initialze a new `Rc`.
    ///
    /// Otherwise, returns `None`.
    fn alloc<F: FnOnce() -> Self::Data>(self: StrongPin<'_, Self>, f: F) -> Option<ArenaRc<Self>>;

    /// Deallocate a given handle, decreasing the reference count
    /// Finalizes the referred object if there are no more handles.
    ///
    /// # Note
    ///
    /// This method is automatically used by the `Rc`.
    /// Usually, you don't need to manually call this method.
    fn dealloc(mut rc: ArenaRc<Self>, ctx: <Self::Data as ArenaObject>::Ctx<'_, '_>) {
        let inner = unsafe { ManuallyDrop::take(&mut rc.inner) };
        if let Ok(mut rm) = inner.into_mut() {
            // Finalize the arena object.
            rm.finalize(ctx);
        }
        core::mem::forget(rc);
    }
}

pub trait ArenaObject {
    type Ctx<'a, 'b: 'a>;

    /// Finalizes the `ArenaObject`.
    /// This function is automatically called when the last `Rc` referring to this `ArenaObject` gets dropped.
    fn finalize<'a, 'b: 'a>(&mut self, ctx: Self::Ctx<'a, 'b>);
}

/// A thread-safe reference counted pointer, allocated from `A: Arena`.
/// The data type is same as `A::Data`.
///
/// # Safety
///
/// * `arena` is pinned.
/// * `inner` is allocated from `arena`.
/// * We can safely dereference `arena` until `inner` gets dropped,
///   because we panic if the arena drops earlier than `inner`.
pub struct ArenaRc<A: Arena> {
    arena: *const A,
    inner: ManuallyDrop<Ref<A::Data>>,
}

impl<T, A: Arena<Data = T>> ArenaRc<A> {
    pub fn new(arena: StrongPin<'_, A>, inner: Ref<A::Data>) -> Self {
        Self {
            arena: arena.as_pin().get_ref(),
            inner: ManuallyDrop::new(inner),
        }
    }
}

// `Rc` is `Send` because it does not impl `DerefMut`,
// and when we access the inner `Arena`, we do it after acquiring `Arena`'s lock.
// Also, `Rc` does not point to thread-local data.
unsafe impl<T: Sync, A: Arena<Data = T>> Send for ArenaRc<A> {}

impl<T, A: Arena<Data = T>> Deref for ArenaRc<A> {
    type Target = T;

    fn deref(&self) -> &T {
        self.inner.deref()
    }
}

impl<A: Arena> Clone for ArenaRc<A> {
    fn clone(&self) -> Self {
        ArenaRc {
            arena: self.arena,
            inner: ManuallyDrop::new(self.inner.deref().clone()),
        }
    }
}

impl<A: Arena> ArenaRc<A> {
    pub fn free(self, ctx: <A::Data as ArenaObject>::Ctx<'_, '_>) {
        A::dealloc(self, ctx);
    }
}

impl<A: Arena> Drop for ArenaRc<A> {
    fn drop(&mut self) {
        panic!();
    }
}
