use core::marker::PhantomData;
use core::mem::{self, ManuallyDrop, MaybeUninit};
use core::ops::{Deref, DerefMut};

use crate::spinlock::Spinlock;

// TODO: We can use min_const_generics feature instead, but recent nightly fails to compile.
const CAPACITY: usize = 100;

struct RcEntry<T> {
    ref_cnt: usize,
    data: MaybeUninit<T>,
}

struct RcPool<T> {
    inner: [RcEntry<T>; CAPACITY],
}

struct UntaggedRc<T> {
    ptr: *mut RcEntry<T>,
}

impl<T> RcPool<T> {
    pub const fn new() -> Self {
        Self {
            inner: [RcEntry {
                ref_cnt: 0,
                data: MaybeUninit::uninit(),
            }; CAPACITY],
        }
    }
}

impl<T> Deref for UntaggedRc<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { (*self.ptr).data.get_ref() }
    }
}

// impl Drop for Rc, panic.
impl<T> Drop for UntaggedRc<T> {
    fn drop(&mut self) {
        panic!("You cannot drop UntaggedRc -- use RcPool::dealloc() instead.");
    }
}

pub trait Pool {
    type Data: 'static;
    type PoolBox: 'static;

    fn alloc(&mut self, val: Self::Data) -> Option<Self::PoolBox>;
    unsafe fn dealloc(&mut self, pbox: Self::PoolBox);
}

impl<T: 'static> Pool for RcPool<T> {
    type Data = T;
    type PoolBox = UntaggedRc<T>;
    fn alloc(&mut self, val: T) -> Option<UntaggedRc<T>> {
        for entry in self.inner.iter_mut() {
            if entry.ref_cnt == 0 {
                entry.data.write(val);
                return Some(UntaggedRc { ptr: entry });
            }
        }

        None
    }

    /// # Safety
    ///  - `rc` must be allocated from `self`.
    unsafe fn dealloc(&mut self, rc: UntaggedRc<T>) {
        let entry = &mut *rc.ptr;
        entry.ref_cnt -= 1;
        if entry.ref_cnt == 0 {
            core::ptr::drop_in_place(&mut entry.data);
        }

        core::mem::forget(rc);
    }
}

/// Allocation from `P`.
#[repr(transparent)]
pub struct TaggedBox<P: PoolRef, A: 'static>
where
    P::P: Pool<PoolBox = A>,
{
    alloc: ManuallyDrop<A>,
    _marker: PhantomData<P>,
}

impl<P: PoolRef, A: 'static> Deref for TaggedBox<P, A>
where
    P::P: Pool<PoolBox = A>,
{
    type Target = A;
    fn deref(&self) -> &Self::Target {
        &self.alloc
    }
}

impl<P: PoolRef, A: 'static> DerefMut for TaggedBox<P, A>
where
    P::P: Pool<PoolBox = A>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.alloc
    }
}

impl<P: PoolRef, A: 'static> Drop for TaggedBox<P, A>
where
    P::P: Pool<PoolBox = A>,
{
    fn drop(&mut self) {
        // SAFETY: We can ensure the box is allocated from `P::REF` by the invariant of PoolRef.
        unsafe {
            let pbox = ManuallyDrop::take(&mut self.alloc);
            P::REF.lock().dealloc(pbox);
        }
    }
}

/// A zero-sized reference of the `Pool`, represented in a type.
///
/// See https://ferrous-systems.com/blog/zero-sized-references/.
///
/// # Safety
/// There should be at most one implementation of PoolRef for each REF.
pub unsafe trait PoolRef: Sized {
    type P: Pool + 'static;
    const REF: &'static Spinlock<Self::P>;

    fn alloc(val: <Self::P as Pool>::Data) -> Option<TaggedBox<Self, <Self::P as Pool>::PoolBox>> {
        let alloc = Self::REF.lock().alloc(val)?;
        Some(TaggedBox {
            alloc: ManuallyDrop::new(alloc),
            _marker: PhantomData,
        })
    }

    fn dealloc(tbox: TaggedBox<Self, <Self::P as Pool>::PoolBox>) {
        mem::drop(tbox);
    }
}
