use core::marker::PhantomData;
use core::ptr::NonNull;

/// `SharedMut` can coexist with `*const` and `*mut`.
///
/// # Safety
///
/// * `ptr` is a valid pointer to a value of `T` that lives for `'a`.
/// * There cannot be multiple `SharedMut`s to the same data.
#[repr(transparent)]
pub struct SharedMut<'a, T> {
    ptr: NonNull<T>,
    _marker: PhantomData<&'a mut T>,
}

impl<'a, T> SharedMut<'a, T> {
    pub fn new(ptr: &'a mut T) -> Self {
        SharedMut {
            ptr: NonNull::from(ptr),
            _marker: PhantomData,
        }
    }

    /// # Safety
    ///
    /// * `ptr` is a valid pointer to `T`.
    /// * There is no existing `&mut`, `&`, or `SharedMut` to the same data.
    pub unsafe fn new_unchecked(ptr: *mut T) -> Self {
        SharedMut {
            // SAFETY: `ptr` is a valid pointer so cannot be null.
            ptr: unsafe { NonNull::new_unchecked(ptr) },
            _marker: PhantomData,
        }
    }

    pub fn ptr(&self) -> NonNull<T> {
        self.ptr
    }

    pub fn as_shared_mut(&mut self) -> SharedMut<'_, T> {
        SharedMut {
            ptr: self.ptr,
            _marker: PhantomData,
        }
    }
}

impl<'a, T, const L: usize> SharedMut<'a, [T; L]> {
    pub fn iter(self) -> IterSharedMut<'a, T> {
        let ptr = self.ptr.as_ptr() as *mut T;
        // SAFETY: `ptr.add(L)` is the end of the array.
        let end = unsafe { ptr.add(L) };
        IterSharedMut {
            ptr,
            end,
            _marker: PhantomData,
        }
    }
}

/// # Safety
///
/// `[ptr..end]` is an array of `T`.
pub struct IterSharedMut<'a, T> {
    ptr: *mut T,
    end: *mut T,
    _marker: PhantomData<&'a mut T>,
}

impl<'a, T: 'a> Iterator for IterSharedMut<'a, T> {
    type Item = SharedMut<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        if core::ptr::eq(self.ptr, self.end) {
            None
        } else {
            let r = SharedMut {
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
