use core::marker::PhantomData;
use core::mem::{self, ManuallyDrop};
use core::ops::Deref;
use core::pin::Pin;
use core::ptr::{self, NonNull};

use pin_project::pin_project;

use crate::list::*;
use crate::lock::{Spinlock, SpinlockGuard};
use crate::pinned_array::IterPinMut;

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

pub struct ArrayEntry<T> {
    refcnt: usize,
    data: T,
}

/// A homogeneous memory allocator equipped with reference counts.
pub struct ArrayArena<T, const CAPACITY: usize> {
    entries: [ArrayEntry<T>; CAPACITY],
}

/// # Safety
///
/// `ptr` is a valid pointer to `ArrayEntry<T>` and has lifetime `'s`.
/// Always acquire the `Spinlock<ArrayArena<T, CAPACITY>>` before modifying `ArrayEntry<T>`.
pub struct ArrayPtr<'s, T> {
    ptr: NonNull<ArrayEntry<T>>,
    _marker: PhantomData<&'s T>,
}

// `ArrayPtr` is `Send` because it does not impl `DerefMut`, and when we access
// the inner `ArrayEntry`, we do it after acquring `ArrayArena`'s lock.
// Also, `ArrayPtr` does not point to thread-local data.
unsafe impl<T: Send> Send for ArrayPtr<'_, T> {}

impl<'s, T> ArrayPtr<'s, T> {
    /// # Safety
    ///
    /// `ptr` should be a valid pointer to `ArrayEntry<T>` and have lifetime `'s`.
    unsafe fn new(ptr: NonNull<ArrayEntry<T>>) -> ArrayPtr<'s, T> {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }
}

#[pin_project]
#[repr(C)]
pub struct MruEntry<T> {
    #[pin]
    list_entry: ListEntry,
    refcnt: usize,
    data: T,
}

/// A homogeneous memory allocator equipped with reference counts.
#[pin_project]
pub struct MruArena<T, const CAPACITY: usize> {
    #[pin]
    entries: [MruEntry<T>; CAPACITY],
    #[pin]
    head: ListEntry,
}

/// # Safety
///
/// `ptr` is a valid pointer to `MruEntry<T>` and has lifetime `'s`.
/// Always acquire the `Spinlock<MruArena<T, CAPACITY>>` before modifying `MruEntry<T>`.
/// Also, never move `MruEntry<T>`.
pub struct MruPtr<'s, T> {
    ptr: NonNull<MruEntry<T>>,
    _marker: PhantomData<&'s T>,
}

/// # Safety
///
/// `inner` is allocated from `tag`
pub struct Rc<'s, A: Arena, T: Deref<Target = A>> {
    tag: T,
    inner: ManuallyDrop<A::Handle<'s>>,
}

impl<T> ArrayEntry<T> {
    pub const fn new(data: T) -> Self {
        Self { refcnt: 0, data }
    }
}

impl<T, const CAPACITY: usize> ArrayArena<T, CAPACITY> {
    // TODO(https://github.com/kaist-cp/rv6/issues/371): unsafe...
    pub const fn new(entries: [ArrayEntry<T>; CAPACITY]) -> Self {
        Self { entries }
    }
}

impl<T> Deref for ArrayPtr<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // It is safe because of the invariant.
        unsafe { &self.ptr.as_ref().data }
    }
}

impl<T> Drop for ArrayPtr<'_, T> {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("ArrayPtr must never drop: use ArrayArena::dealloc instead.");
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
        let mut this = self.lock();

        let mut empty: *mut ArrayEntry<T> = ptr::null_mut();
        for entry in &mut this.entries {
            if entry.refcnt != 0 {
                if c(&entry.data) {
                    entry.refcnt += 1;
                    // It is safe because entry is a part of self, whose lifetime is 's.
                    return Some(unsafe { ArrayPtr::new(NonNull::from(entry)) });
                }
            } else if empty.is_null() {
                empty = entry;
                break;
            }
        }

        if empty.is_null() {
            return None;
        }

        // It is safe because empty is a one of this.entries.
        let entry = unsafe { &mut *empty };
        entry.refcnt = 1;
        n(&mut entry.data);
        // It is safe because entry is a part of self, whose lifetime is 's.
        Some(unsafe { ArrayPtr::new(NonNull::from(entry)) })
    }

    fn alloc_handle<F: FnOnce(&mut Self::Data)>(&self, f: F) -> Option<Self::Handle<'_>> {
        let mut this = self.lock();

        for entry in &mut this.entries {
            if entry.refcnt == 0 {
                entry.refcnt = 1;
                f(&mut entry.data);
                // It is safe because entry is a part of self, whose lifetime is 's.
                return Some(unsafe { ArrayPtr::new(NonNull::from(entry)) });
            }
        }

        None
    }

    /// # Safety
    ///
    /// `handle` must be allocated from `self`.
    unsafe fn dup<'s>(&self, handle: &Self::Handle<'s>) -> Self::Handle<'s> {
        let mut _this = self.lock();

        // TODO(https://github.com/kaist-cp/rv6/issues/369)
        // Make a ArrayArena trait and move this there.
        // It is safe becuase of the invariant of ArrayPtr.
        unsafe { (*handle.ptr.as_ptr()).refcnt += 1 };
        Self::Handle::<'s> {
            ptr: handle.ptr,
            _marker: PhantomData,
        }
    }

    /// # Safety
    ///
    /// `handle` must be allocated from `self`.
    unsafe fn dealloc(&self, mut handle: Self::Handle<'_>) {
        let mut this = self.lock();

        // It is safe becuase of the invariant of ArrayPtr.
        let entry = unsafe { handle.ptr.as_mut() };
        if entry.refcnt == 1 {
            entry.data.finalize::<Self>(&mut this);
        }

        entry.refcnt -= 1;
        mem::forget(handle);
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
    // Assumes `list_entry` is located at the beginning of `MruEntry`.
    const LIST_ENTRY_OFFSET: usize = 0;

    // const LIST_ENTRY_OFFSET: usize = offset_of!(MruEntry<T>, list_entry);

    pub const fn new(data: T) -> Self {
        Self {
            refcnt: 0,
            data,
            list_entry: unsafe { ListEntry::new() },
        }
    }

    pub fn from_list_entry(list_entry: Pin<&mut ListEntry>) -> Pin<&mut Self> {
        let ptr = (list_entry.as_ref().get_ref() as *const _ as usize - Self::LIST_ENTRY_OFFSET)
            as *mut MruEntry<T>;
        unsafe { Pin::new_unchecked(&mut *ptr) }
    }
}

impl<T, const CAPACITY: usize> MruArena<T, CAPACITY> {
    // TODO(https://github.com/kaist-cp/rv6/issues/371): unsafe...
    pub const fn new(entries: [MruEntry<T>; CAPACITY]) -> Self {
        Self {
            entries,
            head: unsafe { ListEntry::new() },
        }
    }

    pub fn init(self: Pin<&mut Self>) {
        let mut this = self.project();

        this.head.as_mut().init();
        let iter: IterPinMut<'_, MruEntry<T>> = this.entries.into();
        for entry in iter {
            this.head.as_mut().prepend(entry.project().list_entry);
        }
    }
}

impl<T> Deref for MruPtr<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &self.ptr.as_ref().data }
    }
}

impl<T> Drop for MruPtr<'_, T> {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("MruPtr must never drop: use MruArena::dealloc instead.");
    }
}

impl<T: 'static + ArenaObject, const CAPACITY: usize> Arena for Spinlock<MruArena<T, CAPACITY>> {
    type Data = T;
    type Guard<'s> = SpinlockGuard<'s, MruArena<T, CAPACITY>>;
    type Handle<'s> = MruPtr<'s, T>;

    fn find_or_alloc_handle<'s, C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &'s self,
        c: C,
        n: N,
    ) -> Option<Self::Handle<'s>> {
        let mut guard = self.lock();
        let this = guard.get_pin_mut().project();
        let head = this.head.as_ref().get_ref() as *const _;

        let mut list_entry = this.head.next();
        let mut empty: *mut MruEntry<T> = ptr::null_mut();
        while list_entry.as_ref().get_ref() as *const _ != head {
            let mut entry = MruEntry::from_list_entry(list_entry.as_mut());
            if c(&entry.as_mut().project().data) {
                *entry.as_mut().project().refcnt += 1;
                return Some(Self::Handle::<'s> {
                    ptr: NonNull::from(entry.as_ref().get_ref()),
                    _marker: PhantomData,
                });
            } else if entry.refcnt == 0 {
                // Safe since `MruPtr` does not move `MruEntry`.
                empty = unsafe { entry.as_mut().get_unchecked_mut() };
            }
            list_entry = list_entry.next();
        }

        if empty.is_null() {
            return None;
        }

        // Safe since `MruPtr` does not move `MruEntry`.
        let entry = unsafe { &mut *empty };
        entry.refcnt = 1;
        n(&mut entry.data);
        Some(Self::Handle::<'s> {
            ptr: NonNull::from(entry),
            _marker: PhantomData,
        })
    }

    fn alloc_handle<'s, F: FnOnce(&mut Self::Data)>(&'s self, f: F) -> Option<Self::Handle<'s>> {
        let mut guard = self.lock();
        let this = guard.get_pin_mut().project();
        let head = this.head.as_ref().get_ref() as *const _;

        let mut list_entry = this.head.prev();
        while list_entry.as_ref().get_ref() as *const _ != head {
            let mut entry = MruEntry::from_list_entry(list_entry.as_mut());
            if *entry.as_mut().project().refcnt == 0 {
                *entry.as_mut().project().refcnt = 1;
                f(&mut entry.as_mut().project().data);
                return Some(Self::Handle::<'s> {
                    // Safe since `MruPtr` does not move `MruEntry`.
                    ptr: NonNull::from(unsafe { entry.as_mut().get_unchecked_mut() }),
                    _marker: PhantomData,
                });
            }
            list_entry = list_entry.prev();
        }

        None
    }

    /// # Safety
    ///
    /// `handle` must be allocated from `self`.
    unsafe fn dup<'s>(&self, handle: &Self::Handle<'s>) -> Self::Handle<'s> {
        let mut _this = self.lock();

        // TODO(https://github.com/kaist-cp/rv6/issues/369)
        // Make a MruArena trait and move this there.
        unsafe { (*handle.ptr.as_ptr()).refcnt += 1 };
        Self::Handle::<'s> {
            ptr: handle.ptr,
            _marker: PhantomData,
        }
    }

    /// # Safety
    ///
    /// `handle` must be allocated from `self`.
    unsafe fn dealloc(&self, mut handle: Self::Handle<'_>) {
        let mut this = self.lock();

        // Safe since we don't move `MruEntry` and access it only through `Pin`.
        // That is, the data is pinned.
        let mut entry = unsafe { Pin::new_unchecked(handle.ptr.as_mut()).project() };
        if *entry.refcnt == 1 {
            entry.data.finalize::<Self>(&mut this);
        }
        *entry.refcnt -= 1;

        if *entry.refcnt == 0 {
            entry.list_entry.as_mut().remove();
            this.get_pin_mut().project().head.prepend(entry.list_entry);
        }

        mem::forget(handle);
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
