//! `StackRc` is a Stack-allocated version of `std::rc::Rc`. It is similar to `std::rc::Rc`, but
//! have a few differences:
//!
//! * `std::rc::Rc` is allocated on heap, while `StackRc` is allocated on stack.
//! * `std::rc::Rc` drops the data on heap when there is no remaining `Rc`. However, `StackRc`
//! drops the data on stack when the stack frame is dropped. If the stack frame is dropped even
//! though there is a remaining `StackRc`, the program panics.
//! * `std::rc::Rc` does not require pinning because there is no way to move the data on heap.
//! However, `StackRc` requires pinning because the data on stack must not move.

use core::cell::Cell;
use core::marker::PhantomPinned;
use core::ops::Deref;
use core::pin::Pin;
use core::ptr::NonNull;

/// # Safety
///
/// `self.rc.get()` equals the number of `StackRc` pointing `self`.
pub struct StackRcBox<T> {
    rc: Cell<usize>,
    value: T,
    _marker: PhantomPinned,
}

impl<T> StackRcBox<T> {
    pub const fn new(value: T) -> Self {
        Self {
            rc: Cell::new(0),
            value,
            _marker: PhantomPinned,
        }
    }

    fn reference_count(&self) -> usize {
        self.rc.get()
    }

    fn set_reference_count(&self, rc: usize) {
        self.rc.set(rc)
    }

    fn inc(&self) {
        let rc = self.reference_count();
        assert_ne!(rc, usize::MAX);
        self.set_reference_count(rc + 1);
    }

    fn dec(&self) {
        let rc = self.reference_count();
        self.set_reference_count(rc - 1);
    }

    /// Returns whether there is a `StackRc` pointing `self`.
    pub fn has_reference(&self) -> bool {
        self.reference_count() > 0
    }
}

impl<T> Drop for StackRcBox<T> {
    fn drop(&mut self) {
        assert_eq!(self.reference_count(), 0);
    }
}

/// # Safety
///
/// `self.ptr` is a mutable pointer to a valid `StackRcBox`.
pub struct StackRc<T> {
    ptr: NonNull<StackRcBox<T>>,
}

impl<T> StackRc<T> {
    /// Constructs a new `StackRc<T>`.
    pub fn new(rc_box: Pin<&mut StackRcBox<T>>) -> Self {
        rc_box.inc();
        Self {
            // SAFETY: `StackRc` never move the box beyond its `ptr`.
            ptr: unsafe { rc_box.get_unchecked_mut() }.into(),
        }
    }

    /// Returns a shared reference to the inner `StackRcBox`.
    #[inline(always)]
    pub fn inner(this: &Self) -> &StackRcBox<T> {
        // SAFETY: invariant of this type
        unsafe { this.ptr.as_ref() }
    }

    /// Returns whether `this` is a unique reference to the inner `StackRcBox`.
    pub fn is_unique(this: &Self) -> bool {
        Self::inner(this).reference_count() == 1
    }

    /// Returns a mutable reference into the given `StackRc`, without any check.
    /// See also `get_mut`, which is safe and does appropriate checks.
    ///
    /// # Safety
    ///
    /// Any other `StackRc` to the same allocation must not be dereferenced for the duration of the
    /// returned borrow. This is trivially the case if no such pointers exist, for example
    /// immediately after `StackRc::new`.
    #[inline]
    pub unsafe fn get_mut_unchecked(this: &mut Self) -> &mut T {
        // SAFETY: safety condition of this method
        unsafe { &mut (*this.ptr.as_ptr()).value }
    }

    /// If there are no other `StackRc` to the same allocation, it applies `f` to a mutable
    /// reference of the inner data and returns the result of the application.
    /// Returns `None` otherwise, because it is not safe to mutate a shared value.
    ///
    /// There is no `get_mut`, which returns a mutable reference to the inner data, because
    /// `get_mut` cannot be safe. The following code must be disallowed:
    ///
    /// ```
    /// let rc_box = StackRcBox::new(0);
    /// let rc1 = StackRc::new(rc_box);
    /// let mref: &mut u32 = rc2.get_mut().unwrap();
    /// let rc2 = StackRc::new(rc_box);
    /// let sref: &u32 = &rc2;
    /// foo(mref, sref); // dangerous!
    /// ```
    ///
    /// `use_mut` does not have such a problem. The following code is safe:
    ///
    /// ```
    /// let rc_box = StackRcBox::new(0);
    /// let rc1 = StackRc::new(rc_box);
    /// rc1.use_mut(...);
    /// let rc2 = StackRc::new(rc_box);
    /// foo(&rc1, &rc2);
    /// ```
    pub fn use_mut<R, F: FnOnce(&mut T) -> R>(this: &mut Self, f: F) -> Option<R> {
        if Self::is_unique(this) {
            // SAFETY: this is a unique reference.
            let res = f(unsafe { Self::get_mut_unchecked(this) });
            Some(res)
        } else {
            None
        }
    }
}

impl<T> Deref for StackRc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &Self::inner(self).value
    }
}

impl<T> Clone for StackRc<T> {
    fn clone(&self) -> Self {
        Self::inner(self).inc();
        Self { ptr: self.ptr }
    }
}

impl<T> Drop for StackRc<T> {
    fn drop(&mut self) {
        Self::inner(self).dec();
    }
}
