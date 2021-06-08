use core::marker::PhantomData;
use core::ops::Deref;
use core::pin::Pin;
use core::ptr::NonNull;

/// # Safety
///
/// * Mutable references to the same data cannot exist until the data is dropped.
#[repr(transparent)]
pub struct StrongPin<'a, T: ?Sized> {
    ptr: &'a T,
}

impl<'a, T> StrongPin<'a, T> {
    /// # Safety
    ///
    /// * Mutable references to the same data cannot exist until the data is dropped.
    pub unsafe fn new_unchecked(ptr: &'a T) -> Self {
        Self { ptr }
    }

    pub fn ptr(&self) -> &'a T {
        self.ptr
    }

    pub fn as_pin(&self) -> Pin<&'a T> {
        unsafe { Pin::new_unchecked(self.ptr) }
    }
}

impl<T: ?Sized> Deref for StrongPin<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.ptr
    }
}

impl<T: ?Sized> Clone for StrongPin<'_, T> {
    fn clone(&self) -> Self {
        Self { ptr: self.ptr }
    }
}

impl<T> Copy for StrongPin<'_, T> {}

/// `StrongPinMut` can coexist with `*const` and `*mut`.
///
/// # Safety
///
/// * `ptr` is a valid pointer to a value of `T` that lives for `'a`.
/// * There cannot be multiple `StrongPinMut`s to the same data.
/// * Mutable references to the same data cannot exist until the data is dropped.
#[repr(transparent)]
pub struct StrongPinMut<'a, T> {
    ptr: NonNull<T>,
    _marker: PhantomData<&'a mut T>,
}

impl<'a, T> StrongPinMut<'a, T> {
    /// # Safety
    ///
    /// * `ptr` is a valid pointer to `T`.
    /// * There is no existing `&mut`, `&`, or `StrongPinMut` to the same data.
    /// * Mutable references to the same data cannot exist until the data is dropped.
    pub unsafe fn new_unchecked(ptr: *mut T) -> Self {
        Self {
            // SAFETY: `ptr` is a valid pointer so cannot be null.
            ptr: unsafe { NonNull::new_unchecked(ptr) },
            _marker: PhantomData,
        }
    }

    pub fn ptr(&self) -> NonNull<T> {
        self.ptr
    }

    pub fn as_mut(&mut self) -> StrongPinMut<'_, T> {
        StrongPinMut {
            ptr: self.ptr,
            _marker: PhantomData,
        }
    }

    pub fn as_ref(&self) -> StrongPin<'a, T> {
        unsafe { StrongPin::new_unchecked(self.ptr.as_ref()) }
    }
}

impl<'a, T, const L: usize> StrongPinMut<'a, [T; L]> {
    pub fn iter_mut(self) -> IterMut<'a, T> {
        let ptr = self.ptr.as_ptr() as *mut T;
        // SAFETY: `ptr.add(L)` is the end of the array.
        let end = unsafe { ptr.add(L) };
        IterMut {
            ptr,
            end,
            _marker: PhantomData,
        }
    }
}

/// # Safety
///
/// `[ptr..end]` is an array of `T`.
pub struct IterMut<'a, T> {
    ptr: *mut T,
    end: *mut T,
    _marker: PhantomData<&'a mut T>,
}

impl<'a, T: 'a> Iterator for IterMut<'a, T> {
    type Item = StrongPinMut<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        if core::ptr::eq(self.ptr, self.end) {
            None
        } else {
            let r = StrongPinMut {
                // SAFETY: invariant
                ptr: unsafe { NonNull::new_unchecked(self.ptr) },
                _marker: PhantomData,
            };
            // SAFETY: invariant
            self.ptr = unsafe { self.ptr.add(1) };
            Some(r)
        }
    }
}
