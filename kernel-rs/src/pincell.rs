use core::fmt::Pointer;
use core::ops::Deref;
use core::pin::Pin;
use core::ptr;

use pin_project::pin_project;

/// `WeakPin<*const T>` or `WeakPin<*mut T>`.
///
/// A shared reference that points to a pinned data and acts like a `*const T` or `*mut T`.
/// However, while dereferencing a `const T`/`*mut T` is always unsafe (even when only immutably dereferencing it),
/// we can safely immutably dereference `WeakPin`s, and especially for `WeakPin<*mut T>`,
/// we can also mutate the value (but only with a restricted API), thanks to the `Pin` contract.
///
/// Also, note that the `WeakPin` implements the `Clone`, `Copy`, and `PartialEq` trait.
#[derive(Clone, Copy)]
pub struct WeakPin<P: Pointer> {
    ptr: P,
}

impl<T> WeakPin<*const T> {
    /// Uninitialized `WeakPin<*const T>`. Never call methods with it.
    pub const unsafe fn zero_ref() -> Self {
        Self { ptr: ptr::null() }
    }

    /// Returns a `WeakPin<*const T>` from a `Pin<&T>`.
    /// This is the only safe way to obtain a `WeakPin<*const T>`.
    // TODO: Change it into `Pin::into_weak()` instead?
    pub fn from_pin_ref(pin: Pin<&T>) -> Self {
        Self {
            ptr: pin.get_ref() as *const _,
        }
    }

    /// # Safety
    ///
    /// Only use if `T` is pinned.
    pub unsafe fn from_raw(ptr: *const T) -> Self {
        Self { ptr }
    }

    pub fn get_ref(&self) -> &T {
        // Safe because of the drop guarantee.
        self.deref()
    }

    pub fn clone(&self) -> Self {
        Self { ptr: self.ptr }
    }
}

impl<T> Deref for WeakPin<*const T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Safe because of the drop guarantee.
        unsafe { &*self.ptr }
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

    /// Returns a `WeakPin<*mut T>` from a `Pin<&mut T>`.
    /// This is the only safe way to obtain a `WeakPin<*mut T>`.
    // TODO: Change it into `Pin::into_weak()` instead?
    pub fn from_pin_mut(pin: Pin<&mut T>) -> Self {
        Self {
            ptr: pin.as_ref().get_ref() as *const _ as *mut _,
        }
    }

    pub fn get_ref(&self) -> &T {
        // Safe because of the drop guarantee.
        self.deref()
    }

    /// Upgrades it into `Pin<&mut T>`.
    ///
    /// # Safety
    ///
    /// Make sure not to leak the `Pin<&mut T>`, or we may end up with multiple `Pin<&mut T>`.
    pub unsafe fn get_unchecked_pin(&mut self) -> Pin<&mut T> {
        unsafe { Pin::new_unchecked(&mut *self.ptr) }
    }

    // Do not use. Use `WeakPin::get_uncheck_pin()`, and `project()` it.
    /// Upgrades it into `&mut T`.
    // pub unsafe fn get_unchecked_mut(&mut self) -> &mut T {
    //     unsafe { &mut *self.ptr }
    // }

    pub fn clone(&self) -> Self {
        Self { ptr: self.ptr }
    }
}

impl<T> Deref for WeakPin<*mut T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Safe because of the drop guarantee.
        unsafe { &*self.ptr }
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

#[pin_project]
pub struct PinCell<T> {
    #[pin]
    data: T,
}

/// A cell that wraps data that should be pinned.
/// This wrapper can have one `Pin<&mut T>` AND/OR multiple shared `WeakPin<*mut T>`.
impl<T> PinCell<T> {
    pub const unsafe fn new_unchecked(data: T) -> Self {
        Self { data }
    }

    // TODO: &mut self? &self?
    pub fn get_mut_pin(self: Pin<&mut Self>) -> Pin<&mut T> {
        self.project().data
    }

    pub fn get_weak_pin(&self) -> WeakPin<*mut T> {
        WeakPin {
            ptr: &self.data as *const _ as *mut _,
        }
    }
}
