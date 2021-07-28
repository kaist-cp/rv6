use crate::addr::{PGSHIFT, PAddr, Addr};

/// Bit position of the page number in PTE.
pub const PTESHIFT: usize = 10;



/// Shift a physical address to the right place for a PTE.
#[inline]
pub fn pa2pte(pa: PAddr) -> usize {
    (pa.into_usize() >> PGSHIFT) << PTESHIFT
}

#[inline]
pub fn pte2pa(pte: usize) -> PAddr {
    ((pte >> PTESHIFT) << PGSHIFT).into()
}
