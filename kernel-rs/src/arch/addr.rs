use core::ops::Add;

use bitflags::bitflags;

/// Bytes per page.
pub const PGSIZE: usize = 4096;

/// Bits of offset within a page.
pub const PGSHIFT: usize = 12;

#[inline]
pub const fn pgroundup(sz: usize) -> usize {
    sz.wrapping_add(PGSIZE).wrapping_sub(1) & !PGSIZE.wrapping_sub(1)
}

#[inline]
pub const fn pgrounddown(a: usize) -> usize {
    a & !PGSIZE.wrapping_sub(1)
}

bitflags! {
    pub struct PteFlags: usize {
        /// valid
        const V = 1 << 0;
        /// readable
        const R = 1 << 1;
        /// writable
        const W = 1 << 2;
        /// executable
        const X = 1 << 3;
        /// user-accessible
        const U = 1 << 4;
    }
}

/// Shift a physical address to the right place for a PTE.
#[inline]
pub fn pa2pte(pa: PAddr) -> usize {
    (pa.into_usize() >> 12) << 10
}

#[inline]
pub fn pte2pa(pte: usize) -> PAddr {
    ((pte >> 10) << 12).into()
}

/// Extract the three 9-bit page table indices from a virtual address.

/// 9 bits
pub const PXMASK: usize = 0x1ff;

#[inline]
pub fn pxshift(level: usize) -> usize {
    PGSHIFT + 9 * level
}

/// One beyond the highest possible virtual address.
/// MAXVA is actually one bit less than the max allowed by
/// Sv39, to avoid having to sign-extend virtual addresses
/// that have the high bit set.
pub const MAXVA: usize = (1) << (9 + 9 + 9 + 12 - 1);

pub trait Addr: Copy + From<usize> + Add<usize, Output = Self> {
    fn into_usize(self) -> usize;
    fn is_null(self) -> bool;
    fn is_page_aligned(self) -> bool;
}

pub trait VAddr: Addr {
    fn px(&self, level: usize) -> usize;
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
            fn px(&self, level: usize) -> usize {
                (self.into_usize() >> pxshift(level)) & PXMASK
            }
        }
    };
}

define_addr_type!(PAddr);
define_addr_type!(KVAddr);
define_addr_type!(UVAddr);

impl_vaddr!(KVAddr);
impl_vaddr!(UVAddr);
