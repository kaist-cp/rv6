use core::fmt::Pointer;
use core::ops::Deref;
use core::pin::Pin;
use core::ptr;
use pin_project::pin_project;

/// `WeakPin<*mut T>`.
/// A shared reference that points to a pinned data and acts like a `*mut T`.
/// However, while dereferencing a `*mut T` is always unsafe, even when only immutably dereferencing it,
/// we can safely immutably dereference the value and also mutate the value (but only with a restricted API)
/// using `WeakPin<*mut T>`s, thanks to the `Pin` contract.
/// Also, note that the `WeakPin<*mut T>` implements the `Clone`, `Copy`, and `PartialEq` trait.
#[derive(Clone, Copy)]
pub struct WeakPin<P: Pointer> {
    ptr: P,
}

impl<T> WeakPin<*mut T> {
    /// Uninitialized `WeakPin<T>`. Never call methods with it.
    pub const unsafe fn zero() -> Self {
        Self {
            ptr: ptr::null_mut(),
        }
    }

    /// Returns a `WeakPin<*mut T>` from a `Pin<&T>`.
    /// This is the only safe way to obtain a `WeakPin<*mut T>`.
    // TODO: Change it into `Pin::into_weak()` instead?
    pub fn from_pin(pin: Pin<&T>) -> Self {
        Self {
            ptr: pin.as_ref().get_ref() as *const _ as *mut _,
        }
    }

    pub fn get_ref(&self) -> &T {
        // Safe because of the drop guarantee.
        self.deref()
    }

    /// Upgrades it into `Pin<&mut T>`.
    pub unsafe fn get_unchecked_pin_mut(&mut self) -> Pin<&mut T> {
        unsafe { Pin::new_unchecked(&mut *self.ptr) }
    }

    /// Upgrades it into `&mut T`.
    pub unsafe fn get_unchecked_mut(&mut self) -> &mut T {
        unsafe { &mut *self.ptr }
    }

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
