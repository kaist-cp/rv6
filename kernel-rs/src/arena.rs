use crate::list::*;
use crate::spinlock::{Spinlock, SpinlockGuard};
use core::marker::PhantomData;
use core::mem::{self, ManuallyDrop};
use core::ops::Deref;
use core::ptr;

/// A homogeneous memory allocator, equipped with the box type representing an allocation.
pub trait Arena: Sized {
    /// The value type of the allocator.
    type Data;

    /// The object handle type of the allocator.
    type Handle;

    /// The guard type for arena.
    type Guard<'s>;

    /// Creates handle from condition without increasing reference count.
    fn unforget<C: Fn(&Self::Data) -> bool>(&self, c: C) -> Option<Self::Handle>;

    /// Find or alloc.
    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Self::Handle>;

    /// Failable allocation.
    fn alloc<F: FnOnce(&mut Self::Data)>(&self, f: F) -> Option<Self::Handle>;

    /// # Safety
    ///
    /// `handle` must be allocated from `self`.
    unsafe fn dup(&self, handle: &Self::Handle) -> Self::Handle;

    /// # Safety
    ///
    /// `pbox` must be allocated from the pool.
    ///
    /// Returns whether the object is finalized.
    unsafe fn dealloc(&self, pbox: Self::Handle);

    fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R;
}

pub trait ArenaObject {
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

pub struct ArrayPtr<T> {
    ptr: *mut ArrayEntry<T>,
    _marker: PhantomData<T>,
}

#[repr(C)]
pub struct MruEntry<T> {
    list_entry: ListEntry,
    refcnt: usize,
    data: T,
}

/// A homogeneous memory allocator equipped with reference counts.
pub struct MruArena<T, const CAPACITY: usize> {
    entries: [MruEntry<T>; CAPACITY],
    head: ListEntry,
}

pub struct MruPtr<T> {
    ptr: *mut MruEntry<T>,
    _marker: PhantomData<T>,
}

pub struct Rc<A: Arena, T: Deref<Target = A>> {
    tag: T,
    inner: ManuallyDrop<<<T as Deref>::Target as Arena>::Handle>,
}

impl<T> ArrayEntry<T> {
    pub const fn new(data: T) -> Self {
        Self { refcnt: 0, data }
    }
}

impl<T, const CAPACITY: usize> ArrayArena<T, CAPACITY> {
    // TODO(rv6): unsafe...
    pub const fn new(entries: [ArrayEntry<T>; CAPACITY]) -> Self {
        Self { entries }
    }
}

impl<T> Deref for ArrayPtr<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &(*self.ptr).data }
    }
}

impl<T> Drop for ArrayPtr<T> {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("ArrayPtr must never drop: use ArrayArena::dealloc instead.");
    }
}

impl<T: 'static + ArenaObject, const CAPACITY: usize> Arena for Spinlock<ArrayArena<T, CAPACITY>> {
    type Data = T;
    type Handle = ArrayPtr<T>;
    type Guard<'s> = SpinlockGuard<'s, ArrayArena<T, CAPACITY>>;

    fn unforget<C: Fn(&Self::Data) -> bool>(&self, c: C) -> Option<Self::Handle> {
        let mut this = self.lock();

        for entry in &mut this.entries {
            if entry.refcnt != 0 && c(&entry.data) {
                return Some(Self::Handle {
                    ptr: entry,
                    _marker: PhantomData,
                });
            }
        }

        None
    }

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Self::Handle> {
        let mut this = self.lock();

        let mut empty: *mut ArrayEntry<T> = ptr::null_mut();
        for entry in &mut this.entries {
            if entry.refcnt != 0 {
                if c(&entry.data) {
                    entry.refcnt += 1;
                    return Some(Self::Handle {
                        ptr: entry,
                        _marker: PhantomData,
                    });
                }
            } else if empty.is_null() {
                empty = entry;
            }
        }

        if empty.is_null() {
            return None;
        }

        let entry = unsafe { &mut *empty };
        entry.refcnt = 1;
        n(&mut entry.data);
        Some(Self::Handle {
            ptr: entry,
            _marker: PhantomData,
        })
    }

    fn alloc<F: FnOnce(&mut T)>(&self, f: F) -> Option<Self::Handle> {
        let mut this = self.lock();

        for entry in &mut this.entries {
            if entry.refcnt == 0 {
                entry.refcnt = 1;
                f(&mut entry.data);
                return Some(Self::Handle {
                    ptr: entry,
                    _marker: PhantomData,
                });
            }
        }

        None
    }

    unsafe fn dup(&self, handle: &Self::Handle) -> Self::Handle {
        let mut _this = self.lock();

        // TODO: Make a ArrayArena trait and move this there.
        (*handle.ptr).refcnt += 1;
        Self::Handle {
            ptr: handle.ptr,
            _marker: PhantomData,
        }
    }

    /// # Safety
    ///
    /// `rc` must be allocated from `self`.
    unsafe fn dealloc(&self, handle: Self::Handle) {
        let mut this = self.lock();

        let entry = &mut *handle.ptr;
        if entry.refcnt == 1 {
            entry.data.finalize::<Self>(&mut this);
        }

        let entry = &mut *handle.ptr;
        entry.refcnt -= 1;
        mem::forget(handle);
    }

    fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        guard.reacquire_after(f)
    }
}

impl<T> MruEntry<T> {
    pub const fn new(data: T) -> Self {
        Self {
            refcnt: 0,
            data,
            list_entry: ListEntry::new(),
        }
    }
}

impl<T, const CAPACITY: usize> MruArena<T, CAPACITY> {
    // TODO(rv6): unsafe...
    pub const fn new(entries: [MruEntry<T>; CAPACITY]) -> Self {
        Self {
            entries,
            head: ListEntry::new(),
        }
    }

    pub fn init(&mut self) {
        self.head.init();

        for entry in &mut self.entries {
            self.head.prepend(&mut entry.list_entry);
        }
    }
}

impl<T> Deref for MruPtr<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &(*self.ptr).data }
    }
}

impl<T> Drop for MruPtr<T> {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("MruPtr must never drop: use MruArena::dealloc instead.");
    }
}

impl<T: 'static + ArenaObject, const CAPACITY: usize> Spinlock<MruArena<T, CAPACITY>> {
    // TODO(rv6): a workarond for https://github.com/Gilnaa/memoffset/issues/49.  Assumes
    // `list_entry` is located at the beginning of `MruEntry`.
    const LIST_ENTRY_OFFSET: usize = 0;
    // const LIST_ENTRY_OFFSET: usize = offset_of!(MruEntry<T>, list_entry);
}

impl<T: 'static + ArenaObject, const CAPACITY: usize> Arena for Spinlock<MruArena<T, CAPACITY>> {
    type Data = T;
    type Handle = MruPtr<T>;
    type Guard<'s> = SpinlockGuard<'s, MruArena<T, CAPACITY>>;

    fn unforget<C: Fn(&Self::Data) -> bool>(&self, c: C) -> Option<Self::Handle> {
        let this = self.lock();

        // Is the block already cached?
        let mut list_entry = this.head.next();
        while list_entry as *const _ != & this.head as *const _ {
            let entry = unsafe {
                &mut *((list_entry as *const _ as usize - Self::LIST_ENTRY_OFFSET) as *mut MruEntry<T>)
            };
            if c(&entry.data) {
                debug_assert!(entry.refcnt != 0);
                return Some(Self::Handle {
                    ptr: entry,
                    _marker: PhantomData,
                });
            }
            list_entry = list_entry.next();
        }

        None
    }

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Self::Handle> {
        // Look through buffer cache for block on device dev.
        // If not found, allocate a buffer.
        // In either case, return locked buffer.

        let this = self.lock();

        // Is the block already cached?
        let mut list_entry = this.head.next();
        let mut empty = ptr::null_mut();
        while list_entry as *const _ != & this.head as *const _ {
            let entry = unsafe {
                &mut *((list_entry as *const _ as usize - Self::LIST_ENTRY_OFFSET) as *mut MruEntry<T>)
            };
            if c(&entry.data) {
                entry.refcnt += 1;
                return Some(Self::Handle {
                    ptr: entry,
                    _marker: PhantomData,
                });
            } else if entry.refcnt == 0 {
                empty = entry;
            }
            list_entry = list_entry.next();
        }

        if empty.is_null() {
            return None;
        }

        let entry = unsafe { &mut *empty };
        entry.refcnt = 1;
        n(&mut entry.data);
        Some(Self::Handle {
            ptr: entry,
            _marker: PhantomData,
        })
    }

    fn alloc<F: FnOnce(&mut T)>(&self, f: F) -> Option<Self::Handle> {
        let this = self.lock();

        let mut list_entry = this.head.prev();
        while list_entry as *const _ != & this.head as *const _ {
            let entry = unsafe {
                &mut *((list_entry as *const _ as usize - Self::LIST_ENTRY_OFFSET) as *mut MruEntry<T>)
            };
            if entry.refcnt == 0 {
                entry.refcnt = 1;
                f(&mut entry.data);
                return Some(Self::Handle {
                    ptr: entry,
                    _marker: PhantomData,
                });
            }
            list_entry = list_entry.prev();
        }

        None
    }

    unsafe fn dup(&self, handle: &Self::Handle) -> Self::Handle {
        let mut _this = self.lock();

        // TODO: Make a MruArena trait and move this there.
        (*handle.ptr).refcnt += 1;
        Self::Handle {
            ptr: handle.ptr,
            _marker: PhantomData,
        }
    }

    /// # Safety
    ///
    /// `rc` must be allocated from `self`.
    unsafe fn dealloc(&self, handle: Self::Handle) {
        let mut this = self.lock();

        let entry = &mut *handle.ptr;
        if entry.refcnt == 1 {
            entry.data.finalize::<Self>(&mut this);
        }

        let entry = &mut *handle.ptr;
        entry.refcnt -= 1;

        if entry.refcnt == 0 {
            entry.list_entry.remove();
            this.head.prepend(&mut entry.list_entry);
        }

        mem::forget(handle);
    }

    fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        guard.reacquire_after(f)
    }
}

impl<A: Arena, T: Deref<Target = A>> Deref for Rc<A, T> {
    type Target = <A as Arena>::Handle;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<A: Arena, T: Deref<Target = A>> Drop for Rc<A, T> {
    fn drop(&mut self) {
        // SAFETY: We can ensure the box is allocated from `self.tag` by the invariant of `Tag`.
        //
        // Drop AFTER the arena guard is dropped, as dropping val may cause the current thread
        // sleep.
        let _val = unsafe { self.tag.dealloc(ManuallyDrop::take(&mut self.inner)) };
    }
}

impl<A: Arena, T: Deref<Target = A>> Rc<A, T> {
    pub unsafe fn from_unchecked(tag: T, inner: <<T as Deref>::Target as Arena>::Handle) -> Self {
        let inner = ManuallyDrop::new(inner);
        Self { tag, inner }
    }
}

impl<A: Arena, T: Clone + Deref<Target = A>> Clone for Rc<A, T> {
    fn clone(&self) -> Self {
        let tag = self.tag.clone();
        let inner = ManuallyDrop::new(unsafe { tag.deref().dup(&self.inner) });
        Self { tag, inner }
    }
}

impl<A: Arena, T: Clone + Deref<Target = A>> Arena for T {
    type Data = A::Data;
    type Handle = Rc<A, T>;
    type Guard<'s> = <A as Arena>::Guard<'s>;

    fn unforget<C: Fn(&Self::Data) -> bool>(&self, c: C) -> Option<Self::Handle> {
        let tag = self.clone();
        let inner = ManuallyDrop::new(tag.deref().unforget(c)?);
        Some(Self::Handle { tag, inner })
    }

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Self::Handle> {
        let tag = self.clone();
        let inner = ManuallyDrop::new(tag.deref().find_or_alloc(c, n)?);
        Some(Self::Handle { tag, inner })
    }

    fn alloc<F: FnOnce(&mut Self::Data)>(&self, f: F) -> Option<Self::Handle> {
        let tag = self.clone();
        let inner = ManuallyDrop::new(tag.deref().alloc(f)?);
        Some(Self::Handle { tag, inner })
    }

    unsafe fn dup(&self, handle: &Self::Handle) -> Self::Handle {
        let tag = self.clone();
        let inner = ManuallyDrop::new(self.deref().dup(&handle.inner));
        Self::Handle { tag, inner }
    }

    /// # Safety
    ///
    /// `pbox` must be allocated from the pool.
    unsafe fn dealloc(&self, mut pbox: Self::Handle) {
        self.deref().dealloc(ManuallyDrop::take(&mut pbox.inner))
    }

    fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        A::reacquire_after(guard, f)
    }
}
