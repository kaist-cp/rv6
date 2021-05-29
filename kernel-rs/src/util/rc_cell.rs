//! Similar to `Arc<T>`, but is not allocated on heap.
//! This type panics if it gets dropped before all `Ref<T>`/`RefMut<T>` drops.
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use super::shared_mut::SharedMut;

const BORROWED_MUT: usize = usize::MAX;

/// # Safety
///
/// * If `refcnt` equals `BORROWED_MUT`, a single `RefMut` refers to `self`.
/// * If `refcnt` equals n where n < `BORROWED_MUT`, n `Ref`s refer to `self`.
/// * `RefMut` can mutate both `data` and `refcnt`.
/// * `Ref` can mutate `refcnt` and read `data`.
pub struct RcCell<T> {
    data: T,
    refcnt: AtomicUsize,
}

/// # Safety
///
/// * It holds a valid pointer.
#[repr(transparent)]
pub struct Ref<T>(NonNull<RcCell<T>>);

/// # Safety
///
/// * It holds a valid pointer.
#[repr(transparent)]
pub struct RefMut<T>(NonNull<RcCell<T>>);

impl<T> RcCell<T> {
    pub const fn new(data: T) -> Self {
        Self {
            data,
            refcnt: AtomicUsize::new(0),
        }
    }

    fn rc(this: SharedMut<'_, Self>) -> &AtomicUsize {
        // SAFETY: invariant of SharedMut
        unsafe { &(*this.ptr().as_ptr()).refcnt }
    }

    pub fn is_borrowed(this: SharedMut<'_, Self>) -> bool {
        Self::rc(this).load(Ordering::Acquire) > 0
    }

    pub fn get_mut(mut this: SharedMut<'_, Self>) -> Option<&mut T> {
        if Self::is_borrowed(this.as_shared_mut()) {
            None
        } else {
            // SAFETY: no `Ref` nor `RefMut` points to `this`.
            Some(unsafe { &mut (*this.ptr().as_ptr()).data })
        }
    }

    pub fn try_borrow(mut this: SharedMut<'_, Self>) -> Option<Ref<T>> {
        loop {
            let r = Self::rc(this.as_shared_mut()).load(Ordering::Acquire);

            if r >= BORROWED_MUT - 1 {
                return None;
            }

            if Self::rc(this.as_shared_mut())
                .compare_exchange(r, r + 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Some(Ref(this.ptr()));
            }
        }
    }

    pub fn borrow(this: SharedMut<'_, Self>) -> Ref<T> {
        Self::try_borrow(this).expect("already mutably borrowed")
    }
}

impl<T> Drop for RcCell<T> {
    fn drop(&mut self) {
        assert!(
            !Self::is_borrowed(SharedMut::new(self)),
            "dropped while borrowed"
        );
    }
}

impl<T> Ref<T> {
    fn rc(&self) -> &AtomicUsize {
        // SAFETY: invariant
        unsafe { &(*self.0.as_ptr()).refcnt }
    }

    pub fn into_mut(self) -> Result<RefMut<T>, Self> {
        if self
            .rc()
            .compare_exchange(1, BORROWED_MUT, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return Err(self);
        }

        let ptr = self.0;
        core::mem::forget(self);
        Ok(RefMut(ptr))
    }
}

impl<T> Deref for Ref<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: `Ref` can read `data`.
        unsafe { &(*self.0.as_ptr()).data }
    }
}

impl<T> Clone for Ref<T> {
    fn clone(&self) -> Self {
        let _ = self.rc().fetch_add(1, Ordering::Relaxed);
        Self(self.0)
    }
}

impl<T> Drop for Ref<T> {
    fn drop(&mut self) {
        let _ = self.rc().fetch_sub(1, Ordering::Release);
    }
}

impl<T> RefMut<T> {
    fn rc(&self) -> &AtomicUsize {
        // SAFETY: invariant
        unsafe { &(*self.0.as_ptr()).refcnt }
    }

    pub fn cell(&self) -> *mut RcCell<T> {
        self.0.as_ptr()
    }
}

impl<T> Deref for RefMut<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: `RefMut` can read `data`.
        unsafe { &(*self.0.as_ptr()).data }
    }
}

impl<T> DerefMut for RefMut<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: `RefMut` can mutate `data`.
        unsafe { &mut (*self.0.as_ptr()).data }
    }
}

impl<T> Drop for RefMut<T> {
    fn drop(&mut self) {
        self.rc().store(0, Ordering::Release);
    }
}
