use core::marker::PhantomData;
use core::mem::{self, ManuallyDrop, MaybeUninit};
use core::ops::{Deref, DerefMut};

use crate::param::NFILE;
use crate::spinlock::Spinlock;

// TODO: We can use min_const_generics feature instead, but recent nightly fails to compile.
const CAPACITY: usize = NFILE;

struct RcEntry<T> {
    ref_cnt: usize,
    data: MaybeUninit<T>,
}

pub struct RcPool<T> {
    inner: [RcEntry<T>; CAPACITY],
}

pub struct UntaggedRc<T> {
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

    // TODO: Make a RcPool trait and move this there.
    pub unsafe fn dup(&mut self, rc: &UntaggedRc<T>) -> UntaggedRc<T> {
        (*rc.ptr).ref_cnt += 1;
        UntaggedRc { ptr: rc.ptr }
    }
}

impl<T> Deref for UntaggedRc<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { (*self.ptr).data.get_ref() }
    }
}

// TODO: This may cause UB; remove after refactoring File::{read, write}.
impl<T> DerefMut for UntaggedRc<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { (*self.ptr).data.get_mut() }
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

    fn alloc(&self, val: Self::Data) -> Option<Self::PoolBox>;
    unsafe fn dealloc(&self, pbox: Self::PoolBox);
}

impl<T: 'static> Pool for Spinlock<RcPool<T>> {
    type Data = T;
    type PoolBox = UntaggedRc<T>;
    fn alloc(&self, val: T) -> Option<UntaggedRc<T>> {
        for entry in self.lock().inner.iter_mut() {
            if entry.ref_cnt == 0 {
                entry.ref_cnt = 1;
                entry.data.write(val);
                return Some(UntaggedRc { ptr: entry });
            }
        }

        None
    }

    /// # Safety
    ///  - `rc` must be allocated from `self`.
    unsafe fn dealloc(&self, rc: UntaggedRc<T>) {
        let val = {
            let _guard = self.lock();
            let entry = &mut *rc.ptr;

            entry.ref_cnt -= 1;
            if entry.ref_cnt == 0 {
                Some(entry.data.read())
            } else {
                None
            }
        };

        mem::forget(rc);

        // Drop AFTER the pool lock is released, as dropping val may cause the current thread sleep.
        mem::drop(val);
    }
}

/// Allocation from `P`.
#[repr(transparent)]
pub struct TaggedBox<P: PoolRef, T: 'static>
where
    P::P: Pool<Data = T>,
{
    alloc: ManuallyDrop<<P::P as Pool>::PoolBox>,
    _marker: PhantomData<P>,
}

impl<P: PoolRef, T: 'static> TaggedBox<P, T>
where
    P::P: Pool<Data = T>,
{
    pub unsafe fn from_unchecked(pbox: <P::P as Pool>::PoolBox) -> Self {
        Self {
            alloc: ManuallyDrop::new(pbox),
            _marker: PhantomData,
        }
    }
}

impl<P: PoolRef, T: 'static> Deref for TaggedBox<P, T>
where
    P::P: Pool<Data = T>,
{
    type Target = <P::P as Pool>::PoolBox;
    fn deref(&self) -> &Self::Target {
        &self.alloc
    }
}

impl<P: PoolRef, T: 'static> DerefMut for TaggedBox<P, T>
where
    P::P: Pool<Data = T>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.alloc
    }
}

impl<P: PoolRef, T: 'static> Drop for TaggedBox<P, T>
where
    P::P: Pool<Data = T>,
{
    fn drop(&mut self) {
        // SAFETY: We can ensure the box is allocated from `P::REF` by the invariant of PoolRef.
        unsafe {
            let pbox = ManuallyDrop::take(&mut self.alloc);
            P::deref().dealloc(pbox);
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

    fn deref() -> &'static Self::P;

    fn alloc(val: <Self::P as Pool>::Data) -> Option<TaggedBox<Self, <Self::P as Pool>::Data>> {
        let alloc = Self::deref().alloc(val)?;
        Some(TaggedBox {
            alloc: ManuallyDrop::new(alloc),
            _marker: PhantomData,
        })
    }

    fn dealloc(tbox: TaggedBox<Self, <Self::P as Pool>::Data>) {
        mem::drop(tbox);
    }
}
