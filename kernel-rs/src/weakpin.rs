//! Like `*const T`/`*mut T`, but can only obtain by consuming a `Pin`.
//!
//! `WeakPin`s are safer than raw pointers but less safe than references.
//! Safer because,
//! * The referent is pinned.
//!   That is, the `WeakPin` is valid until the referent gets drop.
//!   Also, even after the referent gets dropped, if the referent appropriately updated all `WeakPin`s
//!   that used to point to it, the `WeakPin`s are still valid.
//!   To help this, `WeakPin`s are `!Clone`, and can only be obtained in a controlled way.
//! * If you can assume that the `WeakPin` is valid, you only need to worry about the stacked borrow rules.
//!   That is, you at least don't need to worry that the `WeakPin` may not point to a type `T`.
//!   Note that this is possible with raw pointers, such as by type conversion or pointer arithmetic.
//! However, `WeakPin`s are less safe than references, since
//! * You have to manually guarantee that the `WeakPin` is valid.
//! * You have to manually check for the borrow rules.
//!
//! `WeakPin`s are useful when you want to split a struct/array,
//! or when its totally legal to access a specific part of a struct/array according to the stacked borrow rules.
//!
//! e.g. Assume that you want to give a mutable reference of a **part** of a struct to another thread,
//! and will never access that **part** afterwards in this thread. If this is really true, it seems
//! safe for this thread to have a mutable reference of the whole struct while the other thread has a
//! mutable reference of a part of the struct. However, the static borrow rules doesn't allow this.
//! Usually, you should use raw pointers instead, but this creates several unnecessary unsafe blocks.
//! Instead, you could give the other thread a `WeakPin` of a part of the struct,
//! and make the API of the struct safely accept `WeakPin`s.
//!
//! `WeakPin`'s are useful when you want to express a permission level lower than references.
//! Using this, you can express OS concepts, such as processes or wait channels.
//!
//! e.g.`&mut T`: Highest authority. Can mutate any part or call any method.
//!     `&T`    : Medium authority. Can only access any part and can call lesser methods.
//!    `WeakPin`: Lowest authority. Can't access any part and can call even lesser methods.
//!               (e.g. Only carefully allow operations that don't break the stacked borrow rules.)
//!
//! In these ways, you can encapsulate the unsafe blocks inside the type's API,
//! instead of making the unsafe spread all around.

use core::fmt::Pointer;
use core::ops::Deref;
use core::pin::Pin;
use core::ptr;

/// `WeakPin<*const T>`, or `WeakPin<*mut T>`.
/// A pointer that can only be obtained from a `Pin`.
/// These pointers are safer than raw pointers but less safe than references.
/// *See the `WeakPin` module documentation for details.*
pub struct WeakPin<P: Pointer> {
    ptr: P,
}

impl<T> WeakPin<*const T> {
    /// Uninitialized `WeakPin<*const T>`. Never call methods with it.
    pub const unsafe fn zero_ref() -> Self {
        Self { ptr: ptr::null() }
    }

    /// Upgrades it into `Pin<&T>`.
    ///
    /// # Safety
    ///
    /// The caller must manually check that the `WeakPin` is valid.
    /// The caller must manually check for the stacked borrow rules.
    pub unsafe fn get_unchecked_pin(&mut self) -> Pin<&T> {
        Pin::new_unchecked(&*self.ptr)
    }
}

impl<T> From<Pin<&T>> for WeakPin<*const T> {
    //safe?
    fn from(pin: Pin<&T>) -> Self {
        Self {
            ptr: pin.get_ref(),
        }
    }
}

impl<T> PartialEq for WeakPin<*const T> {
    fn eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }
}

impl<T> PartialEq<WeakPin<*mut T>> for WeakPin<*const T> {
    fn eq(&self, other: &WeakPin<*mut T>) -> bool {
        self.ptr == other.ptr as *const T
    }
}

impl<T> WeakPin<*mut T> {
    /// Uninitialized `WeakPin<*mut T>`. Never call methods with it.
    pub const unsafe fn zero_mut() -> Self {
        Self {
            ptr: ptr::null_mut(),
        }
    }

    /// Upgrades it into `Pin<&mut T>`.
    ///
    /// # Safety
    ///
    /// The caller must manually check that the `WeakPin` is valid.
    /// The caller must manually check for the stacked borrow rules.
    pub unsafe fn get_unchecked_pin_mut(&mut self) -> Pin<&mut T> {
        unsafe { Pin::new_unchecked(&mut *self.ptr) }
    }
}

impl<T> From<Pin<&mut T>> for WeakPin<*mut T> {
    //safe? unsafe?
    fn from(pin: Pin<&mut T>) -> Self {
        Self {
            ptr: unsafe { pin.get_unchecked_mut() },
        }
    }
}

impl<T> PartialEq for WeakPin<*mut T> {
    fn eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }
}

impl<T> PartialEq<WeakPin<*const T>> for WeakPin<*mut T> {
    fn eq(&self, other: &WeakPin<*const T>) -> bool {
        self.ptr as *const T == other.ptr
    }
}
