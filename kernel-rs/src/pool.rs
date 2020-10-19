use core::marker::PhantomData;
use core::mem::{self, ManuallyDrop, MaybeUninit};
use core::ops::{Deref, DerefMut};
use core::ptr;

struct RcEntry<T> {
    ref_cnt: usize,
    data: MaybeUninit<T>,
}

/// A homogeneous memory allocator equipped with reference counts.
pub struct RcPool<T, const CAPACITY: usize> {
    inner: [RcEntry<T>; CAPACITY],
}

pub struct UntaggedRc<T> {
    ptr: *mut RcEntry<T>,
}

impl<T, const CAPACITY: usize> RcPool<T, CAPACITY> {
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
    // TODO: Make a RcPool trait and move this there.
    pub unsafe fn dup(&mut self, rc: &UntaggedRc<T>) -> UntaggedRc<T> {
        (*rc.ptr).ref_cnt += 1;
        UntaggedRc { ptr: rc.ptr }
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

// TODO: This may cause UB; remove after refactoring File::{read, write}.
impl<T> DerefMut for UntaggedRc<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { (*self.ptr).data.assume_init_mut() }
    }
}

impl<T> Drop for UntaggedRc<T> {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("UntaggedRc must never drop: use RcPool::dealloc instead.");
    }
}

/// A homogeneous memory allocator, equipped with the box type representing an allocation.
pub trait Pool {
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
    fn alloc<F: FnOnce(*mut Self::Data)>(&mut self, f: F) -> Option<Self::Handle>;

    /// # Safety
    ///
    /// `pbox` must be allocated from the pool.
    unsafe fn dealloc(&mut self, pbox: Self::Handle) -> Option<Self::Data>;
}

impl<T: 'static, const CAPACITY: usize> Pool for RcPool<T, CAPACITY> {
    type Data = T;
    type Handle = UntaggedRc<T>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(*mut Self::Data)>(
        &mut self,
        c: C,
        n: N,
    ) -> Option<Self::Handle> {
        let mut empty: *mut RcEntry<T> = ptr::null_mut();

        for entry in self.inner.iter_mut() {
            if entry.ref_cnt != 0 {
                if c(unsafe { &*entry.data.as_ptr() }) {
                    entry.ref_cnt += 1;
                    return Some(UntaggedRc { ptr: entry });
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
        Some(UntaggedRc { ptr: entry })
    }

    fn alloc<F: FnOnce(*mut T)>(&mut self, f: F) -> Option<UntaggedRc<T>> {
        for entry in self.inner.iter_mut() {
            if entry.ref_cnt == 0 {
                entry.ref_cnt = 1;
                f(entry.data.as_mut_ptr());
                return Some(UntaggedRc { ptr: entry });
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

/// A zero-sized reference of the `Pool`, represented in a type.
///
/// Ensures the safety of `dealloc` by PoolRef type parameter of TaggedRc. See
/// https://ferrous-systems.com/blog/zero-sized-references/ for details.
///
/// # Safety
///
/// There should be at most one implementation of PoolRef for each Pool.
pub unsafe trait PoolRef: Sized {
    type Target: Pool + 'static;
    type Result: DerefMut<Target = Self::Target>;

    fn deref_mut() -> Self::Result;

    fn alloc<F: FnOnce(*mut <Self::Target as Pool>::Data)>(
        f: F,
    ) -> Option<TaggedRc<Self, <Self::Target as Pool>::Data>> {
        let alloc = Self::deref_mut().alloc(f)?;
        Some(TaggedRc {
            alloc: ManuallyDrop::new(alloc),
            _marker: PhantomData,
        })
    }
}

/// Allocation from `P`.
#[repr(transparent)]
pub struct TaggedRc<P: PoolRef, T: 'static>
where
    P::Target: Pool<Data = T>,
{
    alloc: ManuallyDrop<<P::Target as Pool>::Handle>,
    _marker: PhantomData<P>,
}

impl<P: PoolRef, T: 'static> TaggedRc<P, T>
where
    P::Target: Pool<Data = T>,
{
    /// # Safety
    ///
    /// `pbox` must be allocated from `P`.
    pub unsafe fn from_unchecked(pbox: <P::Target as Pool>::Handle) -> Self {
        Self {
            alloc: ManuallyDrop::new(pbox),
            _marker: PhantomData,
        }
    }
}

impl<P: PoolRef, T: 'static> Deref for TaggedRc<P, T>
where
    P::Target: Pool<Data = T>,
{
    type Target = <P::Target as Pool>::Handle;
    fn deref(&self) -> &Self::Target {
        &self.alloc
    }
}

impl<P: PoolRef, T: 'static> DerefMut for TaggedRc<P, T>
where
    P::Target: Pool<Data = T>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.alloc
    }
}

impl<P: PoolRef, T: 'static> Drop for TaggedRc<P, T>
where
    P::Target: Pool<Data = T>,
{
    fn drop(&mut self) {
        // SAFETY: We can ensure the box is allocated from `P` by the invariant of PoolRef.
        unsafe {
            let pbox = ManuallyDrop::take(&mut self.alloc);
            drop(P::deref_mut().dealloc(pbox));
        }
    }
}
