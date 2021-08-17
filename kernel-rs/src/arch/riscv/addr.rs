use crate::addr::{Addr, PAddr, PGSHIFT};

/// Bit position of the page number in PTE.
pub const PTESHIFT: usize = 10;

/// The number of page table levels.
pub const PLNUM: usize = 3;

/// Shift a physical address to the right place for a PTE.
#[inline]
pub fn pa2pte(pa: PAddr) -> usize {
    (pa.into_usize() >> PGSHIFT) << PTESHIFT
}

#[inline]
pub fn pte2pa(pte: usize) -> PAddr {
    ((pte >> PTESHIFT) << PGSHIFT).into()
}
