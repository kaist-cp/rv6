use crate::addr::{Addr, PAddr, PGSHIFT};

/// Bit position of the page number in PTE.
pub const PTESHIFT: usize = 12;

pub const PG_ADDR: usize = 0xFFFFFFFFF000; // bit 47 - bit 12

/// Shift a physical address to the right place for a PTE.
#[inline]
pub fn pa2pte(pa: PAddr) -> usize {
    assert!(pa.into_usize() < (1 << 39));
    (pa.into_usize() >> PGSHIFT) << PTESHIFT
}

#[inline]
pub fn pte2pa(pte: usize) -> PAddr {
    (pte & PG_ADDR).into()
}
