//! `StaticRefCell<T>`.
//! Similar to `RefCell<T>`, but can be stored inside other dynamically checked types without
//! bothering the outer type's dynamic checks.
//!
//! # Storing a dynamically checked type inside a dynamically checked type
//! Both `RefCell<T>`s and `StaticRefCell<T>`s use runtime checks to provide safe interior mutability.
//! However, the two have different semantics when they are stored inside another dynamically checked type.
//! In the `RefCell`'s case, the two dynamic checks become **connected**,
//! but in the `StaticRefCell`'s case, the two dynamic checks remain **independent**.
//!
//!
//! For example, if you store a `RefCell<T>` inside a `Spinlock`, the `SpinlockGuard`
//! can drop only after all `Ref<'s, T>`/`RefMut<'s, T>` drops. Hence, the `Spinlock` maintains
//! locked while borrowing the `RefCell<T>`'s data.
//!
//! On contrast, if you store a `StaticRefCell<T>` inside a `Spinlock`, the `SpinlockGuard`
//! can drop before the `Ref<T>`/`RefMut<T>` drops. Hence, the `Spinlock` does not need to be
//! locked while borrowing the `StaticRefCell<T>`'s data.
//! Note that you can also later acquire a `SpinlockGuard<'s, T>` again and then try to obtain a `RefMut<T>`,
//! but that this will success only if all `Ref<T>`/`RefMut<T>` dropped.
//!
//! Similarly, if you store a `RefCell<T>` inside another `RefCell<U>`, you can't drop a `Ref<'s, U>`
//! while the inner `RefCell<T>` is borrowed. This means you can't mutate the outer `RefCell<U>`'s data.
//! However, if you mutably borrowed the *outer* `RefCell<U>`'s data, you can mutably access the *inner* `RefCell<T>`'s
//! data without going through a runtime check.
//!
//! On contrast, if you stored a `StaticRefCell<T>` inside a `RefCell<U>`, you can drop a `Ref<'s, U>`
//! even if the inner `StaticRefCell` is borrowed. This means you can mutate the outer `RefCell<U>`'s data.
//! However, even if you mutably borrowed the *outer* `RefCell<U>`'s data, you can mutably access the *inner* `StaticRefCell`'s
//! data only after succeeding a runtime check.

use core::cell::{Cell, UnsafeCell};
use core::convert::TryFrom;
use core::marker::PhantomPinned;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;

const BORROWED_MUT: usize = usize::MAX;

/// Similar to `RefCell<T>`, but does not use lifetimes.
/// See the module documentation for details.
pub struct StaticRefCell<T> {
    data: UnsafeCell<T>,
    refcnt: Cell<usize>,
    _pin: PhantomPinned,
}

pub struct Ref<T> {
    ptr: *const StaticRefCell<T>,
}

pub struct RefMut<T> {
    ptr: *const StaticRefCell<T>,
}

impl<T> StaticRefCell<T> {
    /// Returns a new `StaticRefCell<T>` that owns `data`.
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

    /// Immutably borrows the `StaticRefCell` if it is not mutably borrowed.
    /// Otherwise, returns `None`.
    ///
    /// # Note
    ///
    /// `StaticRefCell` allows only up to `usize::MAX` - 1 number of `Ref<T>` to coexist.
    /// Hence, this function will return `None` if the caller tries to borrow more than `usize::MAX` - 1 times.
    pub fn try_borrow(&self) -> Option<Ref<T>> {
        let refcnt = self.refcnt.get();
        if refcnt == BORROWED_MUT - 1 || refcnt == BORROWED_MUT {
            None
        } else {
            self.refcnt.set(self.refcnt.get() + 1);
            Some(Ref { ptr: self })
        }
    }

    /// Mutably borrows the `StaticRefCell` if it is not borrowed.
    /// Otherwise, returns `None`.
    pub fn try_borrow_mut(&self) -> Option<RefMut<T>> {
        if self.is_borrowed() {
            None
        } else {
            self.refcnt.set(BORROWED_MUT);
            Some(RefMut {
                ptr: self as *const _,
            })
        }
    }

    /// Immutably borrows the `StaticRefCell` if it is not mutably borrowed.
    /// Otherwise, panics.
    pub fn borrow(&self) -> Ref<T> {
        self.try_borrow().expect("already mutably borrowed")
    }

    /// Mutably borrows the `StaticRefCell` if it is not borrowed.
    /// Otherwise, panics.
    pub fn borrow_mut(&self) -> RefMut<T> {
        self.try_borrow_mut().expect("already borrowed")
    }
}

impl<T> Drop for StaticRefCell<T> {
    fn drop(&mut self) {
        if self.is_borrowed() {
            panic!("dropped while borrowed");
        }
    }
}

impl<T> Ref<T> {
    /// Returns a raw pointer to the `StaticRefCell` that this `Ref` came from.
    pub fn get_cell(&self) -> *const StaticRefCell<T> {
        self.ptr
    }
}

impl<T> From<RefMut<T>> for Ref<T> {
    fn from(r: RefMut<T>) -> Self {
        let ptr = r.ptr;
        drop(r);
        unsafe {
            (*ptr).refcnt.set(1);
        }
        Self { ptr }
    }
}

impl<T> Clone for Ref<T> {
    fn clone(&self) -> Self {
        let refcnt = unsafe { &(*self.ptr).refcnt };
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

    /// Returns a raw pointer to the `StaticRefCell` that this `RefMut` came from.
    pub fn get_cell(&self) -> *const StaticRefCell<T> {
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
