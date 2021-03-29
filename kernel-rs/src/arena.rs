use core::convert::TryFrom;
use core::marker::PhantomData;
use core::mem::{self, ManuallyDrop};
use core::ops::Deref;
use core::pin::Pin;

use pin_project::pin_project;

use crate::list::*;
use crate::lock::{Spinlock, SpinlockGuard};
use crate::pinned_array::IterPinMut;
use crate::rc_cell::{RcCell, Ref, RefMut};

/// A homogeneous memory allocator, equipped with the box type representing an allocation.
pub trait Arena: Sized {
    /// The value type of the allocator.
    type Data;

    /// The object handle type of the allocator.
    type Handle<'s>;

    /// The guard type for arena.
    type Guard<'s>;

    /// Find or alloc.
    fn find_or_alloc_handle<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Self::Handle<'_>>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Rc<'_, Self, &Self>> {
        let inner = self.find_or_alloc_handle(c, n)?;
        // It is safe becuase inner has been allocated from self.
        Some(unsafe { Rc::from_unchecked(self, inner) })
    }

    /// Failable allocation.
    fn alloc_handle<F: FnOnce(&mut Self::Data)>(&self, f: F) -> Option<Self::Handle<'_>>;

    fn alloc<F: FnOnce(&mut Self::Data)>(&self, f: F) -> Option<Rc<'_, Self, &Self>> {
        let inner = self.alloc_handle(f)?;
        // It is safe becuase inner has been allocated from self.
        Some(unsafe { Rc::from_unchecked(self, inner) })
    }

    /// Duplicate a given handle, and increase the reference count.
    ///
    /// # Safety
    ///
    /// `handle` must be allocated from `self`.
    unsafe fn dup<'s>(&self, handle: &Self::Handle<'s>) -> Self::Handle<'s>;

    /// Deallocate a given handle, and finalize the referred object if there are
    /// no more handles.
    ///
    /// # Safety
    ///
    /// `handle` must be allocated from `self`.
    unsafe fn dealloc(&self, handle: Self::Handle<'_>);

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

/// A homogeneous memory allocator equipped with reference counts.
pub struct ArrayArena<T, const CAPACITY: usize> {
    entries: [RcCell<T>; CAPACITY],
}

/// # Safety
///
/// Always acquire the `Spinlock<ArrayArena<T, CAPACITY>>` before duplicating or finalizing.
pub struct ArrayPtr<'s, T> {
    r: Ref<T>,
    _marker: PhantomData<&'s T>,
}

// `ArrayPtr` is `Send` because it does not impl `DerefMut`, and when we access
// the inner `ArrayEntry`, we do it after acquring `ArrayArena`'s lock.
// Also, `ArrayPtr` does not point to thread-local data.
unsafe impl<T: Send> Send for ArrayPtr<'_, T> {}

#[pin_project]
#[repr(C)]
pub struct MruEntry<T> {
    #[pin]
    list_entry: ListEntry,
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

/// # Safety
///
/// Always acquire the `Spinlock<MruArena<T, CAPACITY>>` before duplicating or finalizing.
pub struct MruPtr<'s, T> {
    r: Ref<T>,
    _marker: PhantomData<&'s T>,
}

/// # Safety
///
/// `inner` is allocated from `tag`
pub struct Rc<'s, A: Arena, T: Deref<Target = A>> {
    tag: T,
    inner: ManuallyDrop<A::Handle<'s>>,
}

impl<T, const CAPACITY: usize> ArrayArena<T, CAPACITY> {
    // TODO(https://github.com/kaist-cp/rv6/issues/371): unsafe...
    pub const fn new(entries: [RcCell<T>; CAPACITY]) -> Self {
        Self { entries }
    }
}

impl<'s, T> ArrayPtr<'s, T> {
    fn new(r: Ref<T>) -> ArrayPtr<'s, T> {
        Self {
            r,
            _marker: PhantomData,
        }
    }
}

impl<T> Deref for ArrayPtr<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.r
    }
}

impl<T: 'static + ArenaObject + Unpin, const CAPACITY: usize> Arena
    for Spinlock<ArrayArena<T, CAPACITY>>
{
    type Data = T;
    type Guard<'s> = SpinlockGuard<'s, ArrayArena<T, CAPACITY>>;
    type Handle<'s> = ArrayPtr<'s, T>;

    fn find_or_alloc_handle<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Self::Handle<'_>> {
        let this = self.lock();

        let mut empty = None;
        for entry in &this.entries {
            if !entry.is_borrowed() {
                if empty.is_none() {
                    empty = Some(entry)
                }
                // Note: Do not use `break` here.
                // We must first search through all entries, and then alloc at empty
                // only if the entry we're finding for doesn't exist.
            } else if let Some(r) = entry.try_borrow() {
                // The entry is not under finalization. Check its data.
                if c(&r) {
                    return Some(ArrayPtr::new(r));
                }
            }
        }

        empty.map(|cell| {
            let mut rm = cell.borrow_mut();
            n(&mut rm);
            ArrayPtr::new(rm.into())
        })
    }

    fn alloc_handle<F: FnOnce(&mut Self::Data)>(&self, f: F) -> Option<Self::Handle<'_>> {
        let this = self.lock();

        for entry in &this.entries {
            if let Some(mut rm) = entry.try_borrow_mut() {
                f(&mut rm);
                return Some(ArrayPtr::new(rm.into()));
            }
        }
        None
    }

    /// # Safety
    ///
    /// `handle` must be allocated from `self`.
    // TODO: If we wrap `ArrayPtr::r` with `SpinlockProtected`, then we can just use `clone` instead.
    unsafe fn dup<'s>(&self, handle: &Self::Handle<'s>) -> Self::Handle<'s> {
        let mut _this = self.lock();
        ArrayPtr::new(handle.r.clone())
    }

    /// # Safety
    ///
    /// `handle` must be allocated from `self`.
    // TODO: If we wrap `ArrayPtr::r` with `SpinlockProtected`, then we can just use `drop` instead.
    unsafe fn dealloc(&self, handle: Self::Handle<'_>) {
        let mut this = self.lock();

        if let Ok(mut rm) = RefMut::<T>::try_from(handle.r) {
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

    /// For the `MruEntry<T>` that corresponds to the given `RefMut<T>`, we move it to the front of the list.
    ///
    /// # Safety
    ///
    /// Only use this if the given `RefMut<T>` was obtained from an `MruEntry<T>`,
    /// which is contained inside the `list`.
    unsafe fn finalize_entry(r: RefMut<T>, list: &List<MruEntry<T>>) {
        let ptr = (r.get_cell() as *const _ as usize - Self::DATA_OFFSET) as *mut MruEntry<T>;
        let entry = unsafe { &*ptr };
        list.push_back(entry);
    }
}

// Safe since `MruEntry` owns a `ListEntry`.
unsafe impl<T> ListNode for MruEntry<T> {
    fn get_list_entry(&self) -> &ListEntry {
        &self.list_entry
    }

    fn from_list_entry(list_entry: *const ListEntry) -> *const Self {
        (list_entry as *const _ as usize - Self::LIST_ENTRY_OFFSET) as *const Self
    }
}

impl<T, const CAPACITY: usize> MruArena<T, CAPACITY> {
    // TODO(https://github.com/kaist-cp/rv6/issues/371): unsafe...
    pub const fn new(entries: [MruEntry<T>; CAPACITY]) -> Self {
        Self {
            entries,
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

impl<'s, T> MruPtr<'s, T> {
    fn new(r: Ref<T>) -> MruPtr<'s, T> {
        Self {
            r,
            _marker: PhantomData,
        }
    }
}

impl<T> Deref for MruPtr<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.r
    }
}

impl<T: 'static + ArenaObject + Unpin, const CAPACITY: usize> Arena
    for Spinlock<MruArena<T, CAPACITY>>
{
    type Data = T;
    type Guard<'s> = SpinlockGuard<'s, MruArena<T, CAPACITY>>;
    type Handle<'s> = MruPtr<'s, T>;

    fn find_or_alloc_handle<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Self::Handle<'_>> {
        let this = self.lock();
        let mut empty = None;
        // Safe since the whole `MruArena` is protected by a lock.
        for entry in unsafe { this.list.iter_unchecked() } {
            if !entry.data.is_borrowed() {
                empty = Some(entry);
            }
            if let Some(r) = entry.data.try_borrow() {
                if c(&r) {
                    return Some(MruPtr::new(r));
                }
            }
        }

        empty.map(|entry| {
            let mut rm = entry.data.borrow_mut();
            n(&mut rm);
            MruPtr::new(rm.into())
        })
    }

    fn alloc_handle<F: FnOnce(&mut Self::Data)>(&self, f: F) -> Option<Self::Handle<'_>> {
        let this = self.lock();
        // Safe since the whole `MruArena` is protected by a lock.
        for entry in unsafe { this.list.iter_unchecked().rev() } {
            if let Some(mut rm) = entry.data.try_borrow_mut() {
                f(&mut rm);
                return Some(MruPtr::new(rm.into()));
            }
        }

        None
    }

    /// # Safety
    ///
    /// `handle` must be allocated from `self`.
    unsafe fn dup<'s>(&self, handle: &Self::Handle<'s>) -> Self::Handle<'s> {
        let mut _this = self.lock();
        MruPtr::new(handle.r.clone())
    }

    /// # Safety
    ///
    /// `handle` must be allocated from `self`.
    unsafe fn dealloc(&self, handle: Self::Handle<'_>) {
        let mut this = self.lock();

        if let Ok(mut rm) = RefMut::<T>::try_from(handle.r) {
            rm.finalize::<Self>(&mut this);
            // Safe since the `handle` was obtained from an `MruEntry`,
            // which is contained inside `&this.list`.
            unsafe { MruEntry::finalize_entry(rm, &this.list) };
        }
    }

    unsafe fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        guard.reacquire_after(f)
    }
}

impl<'s, A: Arena, T: Deref<Target = A>> Deref for Rc<'s, A, T> {
    type Target = A::Handle<'s>;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<'s, A: Arena, T: Deref<Target = A>> Drop for Rc<'s, A, T> {
    fn drop(&mut self) {
        // It is safe because inner is allocated from tag.
        unsafe { self.tag.dealloc(ManuallyDrop::take(&mut self.inner)) };
    }
}

impl<'s, A: Arena, T: Deref<Target = A>> Rc<'s, A, T> {
    /// # Safety
    ///
    /// `inner` must be allocated from `tag`
    pub unsafe fn from_unchecked(tag: T, inner: A::Handle<'s>) -> Self {
        let inner = ManuallyDrop::new(inner);
        Self { tag, inner }
    }
}

impl<'s, A: Arena, T: Clone + Deref<Target = A>> Clone for Rc<'s, A, T> {
    fn clone(&self) -> Self {
        let tag = self.tag.clone();
        // It is safe because inner is allocated from tag.
        let inner = ManuallyDrop::new(unsafe { tag.dup(&self.inner) });
        Self { tag, inner }
    }
}

// Rc is invariant to its lifetime parameter. The reason is that Rc has A::Handle<'s> where A
// implements Arena and A::Handle is an arbitrary type constructor, which should be considered
// invariant. When Rc is instantiated with ArrayArena, A::Handle is ArrayPtr, which is covariant. In
// this case, we want Rc<'b, A, T> <: Rc<'a, A, T>. To make this subtyping possible, we define
// narrow_lifetime to upcast Rc<'b, A, T> to Rc<'a, A, T>. This method can be removed when we remove
// lifetimes from Rc.
// TODO(https://github.com/kaist-cp/rv6/issues/444): remove narrow_lifetime
impl<
        'b,
        T: 'static + ArenaObject + Unpin,
        S: Clone + Deref<Target = Spinlock<ArrayArena<T, CAPACITY>>>,
        const CAPACITY: usize,
    > Rc<'b, Spinlock<ArrayArena<T, CAPACITY>>, S>
{
    pub fn narrow_lifetime<'a>(mut self) -> Rc<'a, Spinlock<ArrayArena<T, CAPACITY>>, S>
    where
        'b: 'a,
    {
        let tag = self.tag.clone();
        let inner = ManuallyDrop::new(unsafe { ManuallyDrop::take(&mut self.inner) });
        mem::forget(self);
        Rc { tag, inner }
    }
}
