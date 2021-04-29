//! The arena module.
//!
//! Includes the `Arena` trait, which represents a type that can be used as an arena.
//! For types that `impl Arena`, you can allocate a thread safe `Rc` (reference counted pointer) from it.
//!
//! This module also includes pre-built arenas, such as `ArrayArena`(array based arena) or `MruArena`(list based arena).
// Note: To let the users implement their own arena types, we need to provide the `Handle::unwrap` method.

use core::mem::ManuallyDrop;
use core::ops::Deref;

use crate::util::{branded::Branded, rc_cell::Ref};

mod array_arena;
mod mru_arena;

pub use array_arena::ArrayArena;
pub use mru_arena::MruArena;

/// A homogeneous memory allocator, equipped with the box type representing an allocation.
pub trait Arena: Sized {
    /// The value type of the allocator.
    type Data: ArenaObject;
    /// The guard type for arena.
    type Guard<'s>;

    /// Find or alloc.
    fn find_or_alloc_handle<'id, C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        self: ArenaRef<'id, &Self>,
        c: C,
        n: N,
    ) -> Option<Handle<'id, Self::Data>>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Rc<Self>> {
        ArenaRef::new(self, |arena| {
            let inner = arena.find_or_alloc_handle(c, n)?;
            Some(Rc::new(arena, inner))
        })
    }

    /// Failable allocation.
    fn alloc_handle<'id, F: FnOnce(&mut Self::Data)>(
        self: ArenaRef<'id, &Self>,
        f: F,
    ) -> Option<Handle<'id, Self::Data>>;

    fn alloc<F: FnOnce(&mut Self::Data)>(&self, f: F) -> Option<Rc<Self>> {
        ArenaRef::new(self, |arena| {
            let inner = arena.alloc_handle(f)?;
            Some(Rc::new(arena, inner))
        })
    }

    /// Duplicate a given handle, and increase the reference count.
    // TODO: If we wrap `ArrayPtr::r` with `RemoteSpinlock`, then we can just use `clone` instead.
    fn dup<'id>(
        self: ArenaRef<'id, &Self>,
        handle: HandleRef<'id, '_, Self::Data>,
    ) -> Handle<'id, Self::Data>;

    /// Deallocate a given handle, and finalize the referred object if there are
    /// no more handles.
    // TODO: If we wrap `ArrayPtr::r` with `RemoteSpinlock`, then we can just use `drop` instead.
    fn dealloc<'id>(self: ArenaRef<'id, &Self>, handle: Handle<'id, Self::Data>);

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

pub trait ArenaObject {
    /// Finalizes the `ArenaObject`.
    /// This function is automatically called when the last `Rc` refereing to this `ArenaObject` gets dropped.
    fn finalize<'s, A: Arena>(&'s mut self, guard: &'s mut A::Guard<'_>);
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

impl<A: Arena> Drop for Rc<A> {
    fn drop(&mut self) {
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };
        self.map_arena(|arena| {
            let inner = Handle(arena.0.brand(inner));
            arena.dealloc(inner);
        });
    }
}

impl<A: Arena> Clone for Rc<A> {
    fn clone(&self) -> Self {
        self.map_arena(|arena| {
            let inner = HandleRef(arena.0.brand(self.inner.deref()));
            Rc::new(arena, arena.dup(inner))
        })
    }
}
