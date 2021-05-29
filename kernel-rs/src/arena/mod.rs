//! The arena module.
//!
//! Includes the `Arena` trait, which represents a type that can be used as an arena.
//! For types that `impl Arena`, you can allocate a thread safe `Rc` (reference counted pointer) from it.
//!
//! This module also includes pre-built arenas, such as `ArrayArena`(array based arena) or `MruArena`(list based arena).
// Note: To let the users implement their own arena types, we may want to add `Rc::new_unchecked` and `Handle::unwrap` method.

use core::mem::ManuallyDrop;
use core::ops::Deref;

use crate::util::{branded::Branded, rc_cell::Ref};

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

    /// Deallocate a given handle, decreasing the reference count
    /// Finalizes the referred object if there are no more handles.
    ///
    /// # Note
    ///
    /// This method is automatically used by the `Rc`.
    /// Usually, you don't need to manually call this method.
    fn dealloc<'id>(self: ArenaRef<'id, &Self>, handle: Handle<'id, Self::Data>) {
        if let Ok(mut rm) = handle.0.into_inner().into_mut() {
            rm.finalize::<Self>();
        }
    }
}

pub trait ArenaObject {
    /// Finalizes the `ArenaObject`.
    /// This function is automatically called when the last `Rc` referring to this `ArenaObject` gets dropped.
    fn finalize<A: Arena>(&mut self);
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

/// An arena handle with an `'id` tag attached.
/// The handle was allocated from an `ArenaRef<'id, &Arena>` that has the same `'id` tag.
pub struct Handle<'id, T>(Branded<'id, Ref<T>>);

/// A branded reference to an arena handle.
/// The handle was allocated from an `ArenaRef<'id, &Arena>` that has the same `'id` tag.
pub struct HandleRef<'id, 's, T>(Branded<'id, &'s Ref<T>>);

impl<'s, T> Deref for HandleRef<'_, 's, T> {
    type Target = Ref<T>;

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
    pub fn new<'id>(arena: ArenaRef<'id, &A>, inner: Handle<'id, T>) -> Self {
        Self {
            arena: arena.0.into_inner(),
            inner: ManuallyDrop::new(inner.0.into_inner()),
        }
    }

    fn map_arena<F: for<'new_id> FnOnce(ArenaRef<'new_id, &A>) -> R, R>(&self, f: F) -> R {
        // SAFETY: Safe because of `Rc`'s invariant.
        Branded::new(unsafe { &*self.arena }, |arena| f(ArenaRef(arena)))
    }
}

impl<T, A: Arena<Data = T>> Deref for Rc<A> {
    type Target = T;

    fn deref(&self) -> &T {
        self.inner.deref()
    }
}

impl<A: Arena> Clone for Rc<A> {
    fn clone(&self) -> Self {
        Rc {
            arena: self.arena,
            inner: ManuallyDrop::new(self.inner.deref().clone()),
        }
    }
}

impl<A: Arena> Drop for Rc<A> {
    fn drop(&mut self) {
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };
        self.map_arena(|arena| {
            let inner = Handle(arena.0.brand(inner));
            arena.dealloc(inner);
        });
    }
}
