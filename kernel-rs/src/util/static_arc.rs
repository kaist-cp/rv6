//! Similar to `Arc<T>`, but is not allocated on heap.
//! This type panics if it gets dropped before all `Ref<T>`/`RefMut<T>` drops.
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use super::strong_pin::StrongPinMut;

const BORROWED_MUT: usize = usize::MAX;

/// # Safety
///
/// * If `refcnt` equals `BORROWED_MUT`, a single `RefMut` refers to `self`.
/// * If `refcnt` equals n where n < `BORROWED_MUT`, n `Ref`s refer to `self`.
/// * `RefMut` can mutate both `data` and `refcnt`.
/// * `Ref` can mutate `refcnt` and read `data`.
pub struct StaticArc<T> {
    data: T,
    refcnt: AtomicUsize,
}

/// # Safety
///
/// * It holds a valid pointer.
#[repr(transparent)]
pub struct Ref<T>(NonNull<StaticArc<T>>);

/// # Safety
///
/// * It holds a valid pointer.
#[repr(transparent)]
pub struct RefMut<T>(NonNull<StaticArc<T>>);

impl<T> StaticArc<T> {
    pub const fn new(data: T) -> Self {
        Self {
            data,
            refcnt: AtomicUsize::new(0),
        }
    }

    #[allow(clippy::needless_lifetimes)]
    fn rc<'s>(self: StrongPinMut<'s, Self>) -> &'s AtomicUsize {
        // SAFETY: invariant of StrongPinMut
        unsafe { &(*self.ptr().as_ptr()).refcnt }
    }

    pub fn is_borrowed(self: StrongPinMut<'_, Self>) -> bool {
        self.rc().load(Ordering::Acquire) > 0
    }

    #[allow(clippy::needless_lifetimes)]
    pub fn get_mut<'s>(mut self: StrongPinMut<'s, Self>) -> Option<&'s mut T> {
        if self.as_mut().is_borrowed() {
            None
        } else {
            // SAFETY: no `Ref` nor `RefMut` points to `self`.
            Some(unsafe { &mut (*self.ptr().as_ptr()).data })
        }
    }

    #[allow(clippy::needless_lifetimes)]
    pub unsafe fn get_mut_unchecked<'s>(self: StrongPinMut<'s, Self>) -> &'s mut T {
        // SAFETY: no `Ref` nor `RefMut` points to `self`.
        unsafe { &mut (*self.ptr().as_ptr()).data }
    }

    pub fn try_borrow(mut self: StrongPinMut<'_, Self>) -> Option<Ref<T>> {
        loop {
            let r = self.as_mut().rc().load(Ordering::Acquire);

            if r >= BORROWED_MUT - 1 {
                return None;
            }

            if self
                .as_mut()
                .rc()
                .compare_exchange(r, r + 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Some(Ref(self.ptr()));
            }
        }
    }

    pub fn borrow(self: StrongPinMut<'_, Self>) -> Ref<T> {
        self.try_borrow().expect("already mutably borrowed")
    }

    pub unsafe fn borrow_unchecked(mut self: StrongPinMut<'_, Self>) -> Ref<T> {
        let _ = self.as_mut().rc().fetch_add(1, Ordering::Relaxed);
        Ref(self.ptr())
    }
}

impl<T> Drop for StaticArc<T> {
    fn drop(&mut self) {
        assert_eq!(
            self.refcnt.load(Ordering::Acquire),
            0,
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

    pub fn cell(&self) -> *mut StaticArc<T> {
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
