use core::ops::{Deref, DerefMut};

use crate::riscv::PGSIZE;

/// Page type.
#[repr(align(4096))]
pub struct Page {
    inner: [u8; PGSIZE],
}

impl Page {
    /// HACK(@efenniht): Workaround for non-const `Default::default`.
    pub const DEFAULT: Self = Self { inner: [0; PGSIZE] };
}

impl Deref for Page {
    type Target = [u8; PGSIZE];
    
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Page {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}