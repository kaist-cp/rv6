use core::{
    mem,
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    ptr,
    ptr::NonNull,
};

use static_assertions::const_assert;

use crate::addr::{PAddr, PGSIZE};

// `RawPage` must be aligned with PGSIZE.
const_assert!(PGSIZE == 4096);

/// Page type.
#[repr(align(4096))]
pub struct RawPage {
    inner: [u8; PGSIZE],
}

/// # Safety
///
/// - inner is 4096 bytes-aligned.
/// - end <= inner < PHYSTOP
/// - Two different pages never overwrap. If p1: Page and p2: Page, then
///   *(p1.inner).inner and *(p1.inner).inner are non-overwrapping arrays.
pub struct Page {
    inner: NonNull<RawPage>,
}

impl RawPage {
    pub fn write_bytes(&mut self, value: u8) {
        unsafe {
            ptr::write_bytes(&mut self.inner, value, 1);
        }
    }
}

impl Deref for RawPage {
    type Target = [u8; PGSIZE];

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for RawPage {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl Page {
    pub fn into_usize(self) -> usize {
        let result = self.inner.as_ptr() as _;
        mem::forget(self);
        result
    }

    pub fn addr(&self) -> PAddr {
        (self.inner.as_ptr() as usize).into()
    }

    /// # Safety
    ///
    /// Given addr must not break the invariant of Page.
    /// - addr is a multiple of PGSIZE.
    /// - end <= addr < PHYSTOP
    /// - If p: Page, then *(p.inner).inner and (addr as *RawPage).inner are
    ///   non-overwrapping arrays.
    pub unsafe fn from_usize(addr: usize) -> Self {
        Self {
            inner: unsafe { NonNull::new_unchecked(addr as *mut _) },
        }
    }

    pub fn as_uninit_mut<T>(&mut self) -> &mut MaybeUninit<T> {
        // TODO(https://github.com/kaist-cp/rv6/issues/471): Use const_assert! (or equivalent)
        // instead. Currently, use of T inside const_assert! incurs a compile error: "can't use
        // generic parameters from outer function". Also, there's a workaround using
        // feature(const_generics) and feature(const_evaluatable_checked). However, using them makes
        // the compiler panic. When the compiler becomes updated, we will fix the following lines to
        // use static checks.
        assert!(mem::size_of::<T>() <= PGSIZE);
        assert_eq!(PGSIZE % mem::align_of::<T>(), 0);

        // SAFETY: self.inner is an array of length PGSIZE aligned with PGSIZE bytes.
        // The above assertions show that it can contain a value of T. As it contains arbitrary
        // data, we cannot treat it as &mut T. Instead, we use &mut MaybeUninit<T>. It's ok because
        // T and MaybeUninit<T> have the same size, alignment, and ABI.
        unsafe { &mut *(self.inner.as_ptr() as *mut MaybeUninit<T>) }
    }
}

impl Deref for Page {
    type Target = RawPage;

    fn deref(&self) -> &Self::Target {
        unsafe { self.inner.as_ref() }
    }
}

impl DerefMut for Page {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.inner.as_mut() }
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("Page must never drop.");
    }
}

pub fn getpagesize() -> usize {
    PGSIZE
}
