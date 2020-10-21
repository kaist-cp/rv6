use core::marker::PhantomData;
use core::mem::{self, ManuallyDrop, MaybeUninit};
use core::ops::{Deref, DerefMut};
use core::ptr;

struct RcEntry<T> {
    ref_cnt: usize,
    data: MaybeUninit<T>,
}

/// A homogeneous memory allocator equipped with reference counts.
pub struct RcArena<T, const CAPACITY: usize> {
    inner: [RcEntry<T>; CAPACITY],
}

pub struct UntaggedRc<T> {
    ptr: *mut RcEntry<T>,
    _marker: PhantomData<T>,
}

impl<T, const CAPACITY: usize> RcArena<T, CAPACITY> {
    pub const fn new() -> Self {
        Self {
            inner: [RcEntry {
                ref_cnt: 0,
                data: MaybeUninit::uninit(),
            }; CAPACITY],
        }
    }

    /// # Safety
    ///
    /// `rc` must be allocated from `self`.
    // TODO: Make a RcArena trait and move this there.
    pub unsafe fn dup(&mut self, rc: &UntaggedRc<T>) -> UntaggedRc<T> {
        (*rc.ptr).ref_cnt += 1;
        UntaggedRc {
            ptr: rc.ptr,
            _marker: PhantomData,
        }
    }
}

impl<T> UntaggedRc<T> {
    pub fn into_raw(self) -> *mut T {
        let result = unsafe { (*self.ptr).data.as_mut_ptr() };
        mem::forget(self);
        result
    }
}

impl<T> Deref for UntaggedRc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { (*self.ptr).data.assume_init_ref() }
    }
}

impl<T> DerefMut for UntaggedRc<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { (*self.ptr).data.assume_init_mut() }
    }
}

impl<T> Drop for UntaggedRc<T> {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("UntaggedRc must never drop: use RcArena::dealloc instead.");
    }
}

/// A homogeneous memory allocator, equipped with the box type representing an allocation.
pub trait Arena {
    /// The value type of the allocator.
    type Data: 'static;

    /// The box type of the allocator.
    type Handle: 'static;

    /// Find or alloc.
    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(*mut Self::Data)>(
        &mut self,
        c: C,
        n: N,
    ) -> Option<Self::Handle>;

    /// Failable allocation.
    fn alloc(&mut self) -> Option<Self::Handle>;

    /// # Safety
    ///
    /// `pbox` must be allocated from the pool.
    unsafe fn dealloc(&mut self, pbox: Self::Handle) -> Option<Self::Data>;
}

impl<T: 'static, const CAPACITY: usize> Arena for RcArena<T, CAPACITY> {
    type Data = T;
    type Handle = UntaggedRc<T>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(*mut Self::Data)>(
        &mut self,
        c: C,
        n: N,
    ) -> Option<Self::Handle> {
        let mut empty: *mut RcEntry<T> = ptr::null_mut();

        for entry in &mut self.inner {
            if entry.ref_cnt != 0 {
                if c(unsafe { &*entry.data.as_ptr() }) {
                    entry.ref_cnt += 1;
                    return Some(UntaggedRc {
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
        entry.ref_cnt = 1;
        n(entry.data.as_mut_ptr());
        Some(UntaggedRc {
            ptr: entry,
            _marker: PhantomData,
        })
    }

    fn alloc(&mut self) -> Option<UntaggedRc<T>> {
        for entry in &mut self.inner {
            if entry.ref_cnt == 0 {
                entry.ref_cnt = 1;
                return Some(UntaggedRc {
                    ptr: entry,
                    _marker: PhantomData,
                });
            }
        }

        None
    }

    /// # Safety
    ///
    /// `rc` must be allocated from `self`.
    unsafe fn dealloc(&mut self, rc: UntaggedRc<T>) -> Option<T> {
        let entry = &mut *rc.ptr;
        entry.ref_cnt -= 1;

        let val = if entry.ref_cnt == 0 {
            Some(entry.data.read())
        } else {
            None
        };

        mem::forget(rc);
        val
    }
}

pub trait Tag: Clone + 'static {
    type Target: Arena;
    type Result: DerefMut<Target = Self::Target>;

    fn arena(&self) -> Self::Result;
}

pub struct Rc<T: Tag> {
    tag: T,
    inner: ManuallyDrop<<<T as Tag>::Target as Arena>::Handle>,
}

impl<A: 'static, T: Tag<Target = RcArena<A, C>>, const C: usize> Deref for Rc<T> {
    type Target = A;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<A: 'static, T: Tag<Target = RcArena<A, C>>, const C: usize> DerefMut for Rc<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}

impl<T: Tag> Drop for Rc<T> {
    fn drop(&mut self) {
        // SAFETY: We can ensure the box is allocated from `self.tag` by the invariant of `Tag`.
        let val = unsafe {
            self.tag
                .arena()
                .dealloc(ManuallyDrop::take(&mut self.inner))
        };

        // Drop AFTER the arena guard is dropped, as dropping val may cause the current thread
        // sleep.
        drop(val);
    }
}

impl<T: Tag> Rc<T> {
    pub unsafe fn from_unchecked(tag: T, inner: <<T as Tag>::Target as Arena>::Handle) -> Self {
        let inner = ManuallyDrop::new(inner);
        Self { tag, inner }
    }
}

impl<A: 'static, T: Tag<Target = RcArena<A, C>>, const C: usize> Clone for Rc<T> {
    fn clone(&self) -> Self {
        let tag = self.tag.clone();
        let inner = ManuallyDrop::new(unsafe { tag.arena().dup(&self.inner) });
        Self { tag, inner }
    }
}

impl<A: Arena + 'static, T: Tag<Target = A>> Arena for T {
    type Data = A::Data;
    type Handle = Rc<T>;

    /// Find or alloc.
    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(*mut Self::Data)>(
        &mut self,
        c: C,
        n: N,
    ) -> Option<Self::Handle> {
        let tag = self.clone();
        let inner = ManuallyDrop::new(tag.arena().find_or_alloc(c, n)?);
        Some(Rc { tag, inner })
    }

    /// Failable allocation.
    fn alloc(&mut self) -> Option<Self::Handle> {
        let tag = self.clone();
        let inner = ManuallyDrop::new(tag.arena().alloc()?);
        Some(Rc { tag, inner })
    }

    /// # Safety
    ///
    /// `pbox` must be allocated from the pool.
    unsafe fn dealloc(&mut self, mut pbox: Self::Handle) -> Option<Self::Data> {
        self.arena().dealloc(ManuallyDrop::take(&mut pbox.inner))
    }
}
