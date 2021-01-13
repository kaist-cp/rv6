use core::{
    mem,
    ops::{Deref, DerefMut},
    ptr,
};

use crate::{println, riscv::PGSIZE, vm::PAddr};

/// Page type.
#[repr(align(4096))]
pub struct RawPage {
    inner: [u8; PGSIZE],
}

// Internal safety invariant:
// - inner is 4096 bytes-aligned.
// - end <= inner < PHYSTOP
// - Two different pages never overwrap. If p1: Page and p2: Page, then
//   *(p1.inner).inner and *(p1.inner).inner are non-overwrapping arrays.
pub struct Page {
    inner: *mut RawPage,
}

impl RawPage {
    /// HACK(@efenniht): Workaround for non-const `Default::default`.
    pub const DEFAULT: Self = Self { inner: [0; PGSIZE] };

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
        let result = self.inner as _;
        mem::forget(self);
        result
    }

    /// # Safety
    ///
    /// Given addr must not break the invariant of Page.
    /// - addr is a multiple of 4096.
    /// - end <= addr < PHYSTOP
    /// - If p: Page, then *(p.inner).inner and (addr as *RawPage).inner are
    ///   non-overwrapping arrays.
    pub unsafe fn from_usize(addr: usize) -> Self {
        Self {
            inner: addr as *mut _,
        }
    }

    pub fn addr(&self) -> PAddr {
        PAddr::new(self.inner as _)
    }
}

impl Deref for Page {
    type Target = RawPage;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.inner }
    }
}

impl DerefMut for Page {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.inner }
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        println!("page must never drop.");
        panic!("Page must never drop.");
    }
}
