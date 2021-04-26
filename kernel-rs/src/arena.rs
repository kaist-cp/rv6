use core::convert::TryFrom;
use core::mem::{self, ManuallyDrop};
use core::ops::Deref;
use core::pin::Pin;

use array_macro::array;
use pin_project::pin_project;

use crate::lock::{Spinlock, SpinlockGuard};
use crate::util::{
    branded::Branded,
    list::{List, ListEntry, ListNode},
    pinned_array::IterPinMut,
    rc_cell::{RcCell, Ref, RefMut},
};

/// A homogeneous memory allocator, equipped with the box type representing an allocation.
pub trait Arena: Sized {
    /// The value type of the allocator.
    type Data: ArenaObject;
    /// The guard type for arena.
    type Guard<'s>;

    /// Creates an `ArenaRef` that wraps `&self` and adds a unique, invariant lifetime `'id`.
    /// The `ArenaRef` can be used only inside the given closure.
    fn map_arena<F: for<'new_id> FnOnce(ArenaRef<'new_id, &Self>) -> R, R>(&self, f: F) -> R {
        Branded::new(self, |arena| f(ArenaRef(arena)))
    }

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
        self.map_arena(|arena| {
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
        self.map_arena(|arena| {
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

/// A homogeneous memory allocator equipped with reference counts.
#[pin_project]
pub struct ArrayArena<T, const CAPACITY: usize> {
    #[pin]
    entries: [RcCell<T>; CAPACITY],
}

#[pin_project]
#[repr(C)]
pub struct MruEntry<T> {
    #[pin]
    list_entry: ListEntry,
    #[pin]
    data: RcCell<T>,
}

/// A homogeneous memory allocator equipped with reference counts.
#[pin_project]
pub struct MruArena<T, const CAPACITY: usize> {
    #[pin]
    entries: [MruEntry<T>; CAPACITY],
    #[pin]
    list: List<MruEntry<T>>,
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
        }
    }
}

impl<T: 'static + ArenaObject + Unpin, const CAPACITY: usize> Arena
    for Spinlock<ArrayArena<T, CAPACITY>>
{
    type Data = T;
    type Guard<'s> = SpinlockGuard<'s, ArrayArena<T, CAPACITY>>;

    fn find_or_alloc_handle<'id, C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        self: ArenaRef<'id, &Self>,
        c: C,
        n: N,
    ) -> Option<Handle<'id, Self::Data>> {
        let mut guard = self.lock();
        let this = guard.get_pin_mut().project();

        let mut empty: Option<*mut RcCell<T>> = None;
        for entry in IterPinMut::from(this.entries) {
            if !entry.is_borrowed() {
                if empty.is_none() {
                    empty = Some(entry.as_ref().get_ref() as *const _ as *mut _)
                }
                // Note: Do not use `break` here.
                // We must first search through all entries, and then alloc at empty
                // only if the entry we're finding for doesn't exist.
            } else if let Some(r) = entry.try_borrow() {
                // The entry is not under finalization. Check its data.
                if c(&r) {
                    return Some(Handle(self.0.brand(r)));
                }
            }
        }

        empty.map(|cell_raw| {
            // SAFETY: `cell` is not referenced or borrowed. Also, it is already pinned.
            let mut cell = unsafe { Pin::new_unchecked(&mut *cell_raw) };
            n(cell.as_mut().get_pin_mut().unwrap().get_mut());
            Handle(self.0.brand(cell.borrow()))
        })
    }

    fn alloc_handle<'id, F: FnOnce(&mut Self::Data)>(
        self: ArenaRef<'id, &Self>,
        f: F,
    ) -> Option<Handle<'id, Self::Data>> {
        let mut guard = self.lock();
        let this = guard.get_pin_mut().project();

        for mut entry in IterPinMut::from(this.entries) {
            if !entry.is_borrowed() {
                f(entry.as_mut().get_pin_mut().unwrap().get_mut());
                return Some(Handle(self.0.brand(entry.borrow())));
            }
        }
        None
    }

    fn dup<'id>(
        self: ArenaRef<'id, &Self>,
        handle: HandleRef<'id, '_, Self::Data>,
    ) -> Handle<'id, Self::Data> {
        let mut _this = self.lock();
        Handle(self.0.brand(handle.0.into_inner().clone()))
    }

    fn dealloc<'id>(self: ArenaRef<'id, &Self>, handle: Handle<'id, Self::Data>) {
        let mut this = self.lock();

        if let Ok(mut rm) = RefMut::<T>::try_from(handle.0.into_inner()) {
            rm.finalize::<Self>(&mut this);
        }
    }

    unsafe fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        guard.reacquire_after(f)
    }
}

impl<T> MruEntry<T> {
    // TODO(https://github.com/kaist-cp/rv6/issues/369)
    // A workarond for https://github.com/Gilnaa/memoffset/issues/49.
    // Assumes `list_entry` is located at the beginning of `MruEntry`
    // and `data` is located at `mem::size_of::<ListEntry>()`.
    const DATA_OFFSET: usize = mem::size_of::<ListEntry>();
    const LIST_ENTRY_OFFSET: usize = 0;

    // const DATA_OFFSET: usize = offset_of!(MruEntry<T>, data);
    // const LIST_ENTRY_OFFSET: usize = offset_of!(MruEntry<T>, list_entry);

    pub const fn new(data: T) -> Self {
        Self {
            list_entry: unsafe { ListEntry::new() },
            data: RcCell::new(data),
        }
    }
}

// SAFETY: `MruEntry` owns a `ListEntry`.
unsafe impl<T> ListNode for MruEntry<T> {
    fn get_list_entry(&self) -> &ListEntry {
        &self.list_entry
    }

    fn from_list_entry(list_entry: *const ListEntry) -> *const Self {
        (list_entry as *const _ as usize - Self::LIST_ENTRY_OFFSET) as *const Self
    }
}

impl<T, const CAPACITY: usize> MruArena<T, CAPACITY> {
    /// Returns an `MruArena` of size `CAPACITY` that is filled with `D`'s const default value.
    /// Note that `D` must `impl const Default`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let mru_arena = MruArena::<D, 100>::new();
    /// ```
    // Note: We cannot use the generic `T` in the following function, since we need to only allow
    // types that `impl const Default`, not just `impl Default`.
    #[allow(clippy::new_ret_no_self)]
    pub const fn new<D: Default>() -> MruArena<D, CAPACITY> {
        MruArena {
            entries: array![_ => MruEntry::new(Default::default()); CAPACITY],
            list: unsafe { List::new() },
        }
    }

    pub fn init(self: Pin<&mut Self>) {
        let mut this = self.project();
        this.list.as_mut().init();
        for mut entry in IterPinMut::from(this.entries) {
            entry.as_mut().project().list_entry.init();
            this.list.push_front(&entry);
        }
    }
}

impl<T: 'static + ArenaObject + Unpin, const CAPACITY: usize> Arena
    for Spinlock<MruArena<T, CAPACITY>>
{
    type Data = T;
    type Guard<'s> = SpinlockGuard<'s, MruArena<T, CAPACITY>>;

    fn find_or_alloc_handle<'id, C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        self: ArenaRef<'id, &Self>,
        c: C,
        n: N,
    ) -> Option<Handle<'id, Self::Data>> {
        let mut guard = self.lock();
        let this = guard.get_pin_mut().project();

        let mut empty: Option<*mut RcCell<T>> = None;
        // SAFETY: the whole `MruArena` is protected by a lock.
        for entry in unsafe { this.list.iter_pin_mut_unchecked() } {
            if !entry.data.is_borrowed() {
                empty = Some(&entry.data as *const _ as *mut _);
            }
            if let Some(r) = entry.data.try_borrow() {
                if c(&r) {
                    return Some(Handle(self.0.brand(r)));
                }
            }
        }

        empty.map(|cell_raw| {
            // SAFETY: `cell` is not referenced or borrowed. Also, it is already pinned.
            let mut cell = unsafe { Pin::new_unchecked(&mut *cell_raw) };
            n(cell.as_mut().get_pin_mut().unwrap().get_mut());
            Handle(self.0.brand(cell.borrow()))
        })
    }

    fn alloc_handle<'id, F: FnOnce(&mut Self::Data)>(
        self: ArenaRef<'id, &Self>,
        f: F,
    ) -> Option<Handle<'id, Self::Data>> {
        let mut guard = self.lock();
        let this = guard.get_pin_mut().project();

        // SAFETY: the whole `MruArena` is protected by a lock.
        for mut entry in unsafe { this.list.iter_pin_mut_unchecked().rev() } {
            if !entry.data.is_borrowed() {
                f(entry
                    .as_mut()
                    .project()
                    .data
                    .get_pin_mut()
                    .unwrap()
                    .get_mut());
                return Some(Handle(self.0.brand(entry.data.borrow())));
            }
        }

        None
    }

    fn dup<'id>(
        self: ArenaRef<'id, &Self>,
        handle: HandleRef<'id, '_, Self::Data>,
    ) -> Handle<'id, Self::Data> {
        let mut _this = self.lock();
        Handle(self.0.brand(handle.0.into_inner().clone()))
    }

    fn dealloc<'id>(self: ArenaRef<'id, &Self>, handle: Handle<'id, Self::Data>) {
        let mut this = self.lock();

        if let Ok(mut rm) = RefMut::<T>::try_from(handle.0.into_inner()) {
            // Finalize the arena object.
            rm.finalize::<Self>(&mut this);

            // Move this entry to the back of the list.
            let ptr = (rm.get_cell() as *const _ as usize - MruEntry::<T>::DATA_OFFSET)
                as *mut MruEntry<T>;
            let entry = unsafe { &*ptr };
            this.list.push_back(entry);
        }
    }

    unsafe fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        guard.reacquire_after(f)
    }
}

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
