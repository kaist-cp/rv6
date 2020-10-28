use crate::spinlock::{Spinlock, SpinlockGuard};
use core::marker::PhantomData;
use core::mem::{self, ManuallyDrop};
use core::ops::Deref;
use core::ptr;

pub struct RcEntry<T> {
    refcnt: usize,
    data: T,
}

/// A homogeneous memory allocator equipped with reference counts.
pub struct RcArena<T, const CAPACITY: usize> {
    inner: [RcEntry<T>; CAPACITY],
}

pub struct UntaggedRc<T> {
    ptr: *mut RcEntry<T>,
    _marker: PhantomData<T>,
}

pub struct Rc<A: Arena, T: Deref<Target = A>> {
    tag: T,
    inner: ManuallyDrop<<<T as Deref>::Target as Arena>::Handle>,
}

/// A homogeneous memory allocator, equipped with the box type representing an allocation.
pub trait Arena: Sized {
    /// The value type of the allocator.
    type Data;

    /// The object handle type of the allocator.
    type Handle;

    /// The guard type for arena.
    type Guard<'s>;

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
    unsafe fn dealloc(&self, pbox: Self::Handle) -> bool;

    fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R;
}

pub trait ArenaObject {
    fn finalize<'s, A: Arena>(&'s mut self, guard: &'s mut A::Guard<'_>);
}

impl<T> RcEntry<T> {
    pub const fn new(data: T) -> Self {
        Self { refcnt: 0, data }
    }
}

impl<T, const CAPACITY: usize> RcArena<T, CAPACITY> {
    // TODO(rv6): unsafe...
    pub const fn new(inner: [RcEntry<T>; CAPACITY]) -> Self {
        Self { inner }
    }
}

impl<T> Deref for UntaggedRc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &(*self.ptr).data }
    }
}

impl<T> Drop for UntaggedRc<T> {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("UntaggedRc must never drop: use RcArena::dealloc instead.");
    }
}

impl<T: 'static + ArenaObject, const CAPACITY: usize> Arena for Spinlock<RcArena<T, CAPACITY>> {
    type Data = T;
    type Handle = UntaggedRc<T>;
    type Guard<'s> = SpinlockGuard<'s, RcArena<T, CAPACITY>>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Self::Handle> {
        let mut this = self.lock();

        let mut empty: *mut RcEntry<T> = ptr::null_mut();
        for entry in &mut this.inner {
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

        for entry in &mut this.inner {
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
        // TODO: Make a RcArena trait and move this there.
        (*handle.ptr).refcnt += 1;
        Self::Handle {
            ptr: handle.ptr,
            _marker: PhantomData,
        }
    }

    /// # Safety
    ///
    /// `rc` must be allocated from `self`.
    unsafe fn dealloc(&self, handle: Self::Handle) -> bool {
        let mut this = self.lock();

        let entry = &mut *handle.ptr;
        let refcnt = entry.refcnt - 1;

        let result = if refcnt == 0 {
            entry.data.finalize::<Self>(&mut this);
            true
        } else {
            false
        };

        entry.refcnt = refcnt;
        mem::forget(handle);
        result
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

    /// Find or alloc.
    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Self::Handle> {
        let tag = self.clone();
        let inner = ManuallyDrop::new(tag.deref().find_or_alloc(c, n)?);
        Some(Self::Handle { tag, inner })
    }

    /// Failable allocation.
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
    unsafe fn dealloc(&self, mut pbox: Self::Handle) -> bool {
        self.deref().dealloc(ManuallyDrop::take(&mut pbox.inner))
    }

    fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        A::reacquire_after(guard, f)
    }
}
