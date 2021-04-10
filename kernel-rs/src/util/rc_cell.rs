//! Similar to `RefCell<T>`, but provides lifetime-less `Ref<T>` and `RefMut<T>`.
//! Instead, this type panics if it gets dropped before all `Ref<T>`/`RefMut<T>` drops.
//! Also, this type must be pinned at all time.
//!
//! # Storing a dynamically checked type inside a dynamically checked type
//! `RefCell` and `RcCell` have different semantics when they are stored inside another dynamically checked type.
//! In the `RefCell`'s case, the two dynamic checks become **connected**,
//! but in the `RcCell`'s case, the two dynamic checks remain **independent**.
//!
//! ## Example 1: `Spinlock<RefCell<T>>` vs `Spinlock<RcCell<T>>`
//! Let us compare `Spinlock<RefCell<T>>` vs `Spinlock<RcCell<T>>`.
//! Suppose you acquired the `Spinlock`, and then borrowed the inner `RefCell<T>`/`RcCell<T>` using the guard.
//! * For `Spinlock<RefCell<T>>`, the `Spinlock`'s guard cannot drop until all `Ref`/`RefMut` borrowed
//! from the inner `RefCell<T>` drops.
//! * In contrast, for `Spinlock<RcCell<T>>`, the `Spinlock`'s guard can drop before all `Ref`/`RefMut` drops.
//!
//! This is because while `RefCell` provides `Ref<'s, T>`/`RefMut<'s, T>` which borrows the `RefCell` for their
//! whole lifetime, `RcCell` provides `Ref<T>`/`RefMut<T>` which does not borrow the `RcCell`.
//!
//! ## Example 2: `RefCell<(RefCell<T>, U)>` vs `RefCell<(RcCell<T>, U)>`
//! Similarly, let us compare `RefCell<(RefCell<T>, U)>` vs `RefCell<(RcCell<T>, U)>`.
//! Suppose you immutably borrowed the outer `RefCell` and obtained an `Ref`.
//! Then, using it, suppose you mutably borrowed the inner `RefCell`/`RcCell` and obtained an `RefMut`.
//! * For `RefCell<(RefCell<T>, U)>`, the `Ref` cannot drop until the `RefMut` drops.
//! That is, you cannot mutate the `U` while mutating the `T`.
//! * In contrast, for `RefCell<(RcCell<T>, U)>`, the `Ref` can drop before the `RefMut` drops.
//! That is, you can mutate the `U` while mutating the `T`.

use core::cell::{Cell, UnsafeCell};
use core::convert::TryFrom;
use core::marker::PhantomPinned;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;

const BORROWED_MUT: usize = usize::MAX;

/// Similar to `RefCell<T>`, but provides lifetime-less `Ref<T>` and `RefMut<T>`.
/// See the module documentation for details.
pub struct RcCell<T> {
    data: UnsafeCell<T>,
    refcnt: Cell<usize>,
    _pin: PhantomPinned,
}

/// A lifetme-less wrapper type for an immutably borrowed value from a RcCell<T>.
pub struct Ref<T> {
    ptr: *const RcCell<T>,
}

/// A lifetme-less wrapper type for a mutably borrowed value from a RcCell<T>.
pub struct RefMut<T> {
    ptr: *const RcCell<T>,
}

impl<T> RcCell<T> {
    /// Returns a new `RcCell<T>` that owns `data`.
    pub const fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            refcnt: Cell::new(0),
            _pin: PhantomPinned,
        }
    }

    /// Returns true if its borrowed immutably or mutably.
    pub fn is_borrowed(&self) -> bool {
        self.refcnt.get() != 0
    }

    /// Returns true if its mutably borrowed.
    pub fn is_borrowed_mut(&self) -> bool {
        self.refcnt.get() == BORROWED_MUT
    }

    /// Returns a raw pointer to the inner data.
    pub fn as_ptr(&self) -> *mut T {
        self.data.get()
    }

    /// If the `RcCell` is not borrowed, returns a pinned mutable reference to the underlying data.
    /// Otherwise, returns `None`.
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Option<Pin<&mut T>> {
        if self.is_borrowed() {
            None
        } else {
            // SAFETY: `self` is pinned.
            Some(unsafe { Pin::new_unchecked(&mut *self.data.get()) })
        }
    }

    /// Immutably borrows the `RcCell` if it is not mutably borrowed.
    /// Otherwise, returns `None`.
    ///
    /// # Note
    ///
    /// `RcCell` allows only up to `usize::MAX - 1` number of `Ref<T>` to coexist.
    /// Hence, this function will return `None` if the caller tries to borrow more than `usize::MAX - 1` times.
    pub fn try_borrow(&self) -> Option<Ref<T>> {
        let refcnt = self.refcnt.get();
        if refcnt == BORROWED_MUT - 1 || refcnt == BORROWED_MUT {
            None
        } else {
            self.refcnt.set(refcnt + 1);
            Some(Ref { ptr: self })
        }
    }

    /// Mutably borrows the `RcCell` if it is not borrowed.
    /// Otherwise, returns `None`.
    pub fn try_borrow_mut(&self) -> Option<RefMut<T>> {
        if self.is_borrowed() {
            None
        } else {
            self.refcnt.set(BORROWED_MUT);
            Some(RefMut { ptr: self })
        }
    }

    /// Immutably borrows the `RcCell` if it is not mutably borrowed.
    /// Otherwise, panics.
    pub fn borrow(&self) -> Ref<T> {
        self.try_borrow().expect("already mutably borrowed")
    }

    /// Mutably borrows the `RcCell` if it is not borrowed.
    /// Otherwise, panics.
    pub fn borrow_mut(&self) -> RefMut<T> {
        self.try_borrow_mut().expect("already borrowed")
    }
}

impl<T> Drop for RcCell<T> {
    fn drop(&mut self) {
        assert!(!self.is_borrowed(), "dropped while borrowed");
    }
}

impl<T> From<RefMut<T>> for Ref<T> {
    fn from(r: RefMut<T>) -> Self {
        let ptr = r.ptr;
        drop(r);
        unsafe { (*ptr).refcnt.set(1) };
        Self { ptr }
    }
}

impl<T> Clone for Ref<T> {
    fn clone(&self) -> Self {
        let refcnt = unsafe { &(*self.ptr).refcnt };
        assert!(refcnt.get() != BORROWED_MUT - 1, "borrowed too many times");
        refcnt.set(refcnt.get() + 1);
        Self { ptr: self.ptr }
    }
}

impl<T> Deref for Ref<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(*self.ptr).data.get() }
    }
}

impl<T> Drop for Ref<T> {
    fn drop(&mut self) {
        let refcnt = unsafe { &(*self.ptr).refcnt };
        debug_assert!(refcnt.get() != 0 && refcnt.get() != BORROWED_MUT);
        refcnt.set(refcnt.get() - 1);
    }
}

impl<T> RefMut<T> {
    /// Returns a pinned mutable reference to the inner data.
    pub fn get_pin_mut(&mut self) -> Pin<&mut T> {
        // TODO: Add safety reasoning after fixing issue #439
        unsafe { Pin::new_unchecked(&mut *(*self.ptr).data.get()) }
    }

    /// Returns a raw pointer to the `RcCell` that this `RefMut` came from.
    pub fn get_cell(&self) -> *const RcCell<T> {
        self.ptr
    }
}

impl<T> TryFrom<Ref<T>> for RefMut<T> {
    type Error = ();

    fn try_from(r: Ref<T>) -> Result<Self, Self::Error> {
        let refcnt = unsafe { &(*r.ptr).refcnt };
        if refcnt.get() == 1 {
            let ptr = r.ptr;
            drop(r);
            refcnt.set(BORROWED_MUT);
            Ok(RefMut { ptr })
        } else {
            Err(())
        }
    }
}

impl<T> Deref for RefMut<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(*self.ptr).data.get() }
    }
}

// If `T: !Unpin`, we should not be able to obtain a mutable reference to the inner data.
// Hence, `RefMut<T>` implements `DerefMut` only when `T: Unpin`.
// Use `RefMut::get_pin_mut` instead when `T: !Unpin`.
impl<T: Unpin> DerefMut for RefMut<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_pin_mut().get_mut()
    }
}

impl<T> Drop for RefMut<T> {
    fn drop(&mut self) {
        unsafe {
            debug_assert!((*self.ptr).refcnt.get() == BORROWED_MUT);
            (*self.ptr).refcnt.set(0);
        }
    }
}
