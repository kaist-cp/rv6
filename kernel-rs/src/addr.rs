use core::ops::Add;
pub use crate::arch::addr::*;

/// Bits of offset within a page.
pub const PGSHIFT: usize = 12;

/// Bytes per page.
pub const PGSIZE: usize = 1 << PGSHIFT;

/// Bits of offset for each page table level.
pub const PLSHIFT: usize = 9;

/// Bytes per page table level.
pub const PLSIZE: usize = 1 << PLSHIFT;

/// Bit mask for page table index.
pub const PLMASK: usize = PLSIZE - 1;

/// The number of page table levels.
pub const PLNUM: usize = 3;

/// One beyond the highest possible virtual address.
/// MAXVA is actually one bit less than the max allowed by
/// Sv39, to avoid having to sign-extend virtual addresses
/// that have the high bit set.
pub const MAXVA: usize = 1 << (PLSHIFT * PLNUM + PGSHIFT - 1);

#[inline]
pub const fn pgroundup(sz: usize) -> usize {
    sz.wrapping_add(PGSIZE).wrapping_sub(1) & !PGSIZE.wrapping_sub(1)
}

#[inline]
pub const fn pgrounddown(a: usize) -> usize {
    a & !PGSIZE.wrapping_sub(1)
}

pub trait Addr: Copy + From<usize> + Add<usize, Output = Self> {
    fn into_usize(self) -> usize;
    fn is_null(self) -> bool;
    fn is_page_aligned(self) -> bool;
}

pub trait VAddr: Addr {
    fn page_table_index(&self, level: usize) -> usize;
}

macro_rules! define_addr_type {
    ($typ:ident) => {
        #[derive(Clone, Copy)]
        pub struct $typ(usize);

        impl From<usize> for $typ {
            fn from(value: usize) -> Self {
                Self(value)
            }
        }

        impl Add<usize> for $typ {
            type Output = Self;

            fn add(self, rhs: usize) -> Self::Output {
                Self(self.0 + rhs)
            }
        }

        impl Addr for $typ {
            fn into_usize(self) -> usize {
                self.0
            }

            fn is_null(self) -> bool {
                self.0 == 0
            }

            fn is_page_aligned(self) -> bool {
                self.0 % PGSIZE == 0
            }
        }
    };
}

macro_rules! impl_vaddr {
    ($typ:ident) => {
        impl VAddr for $typ {
            fn page_table_index(&self, level: usize) -> usize {
                (self.into_usize() >> (PGSHIFT + PLSHIFT * level)) & PLMASK
            }
        }
    };
}

define_addr_type!(PAddr);
define_addr_type!(KVAddr);
define_addr_type!(UVAddr);

impl_vaddr!(KVAddr);
impl_vaddr!(UVAddr);
