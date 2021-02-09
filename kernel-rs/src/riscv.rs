use bitflags::bitflags;

use crate::vm::{Addr, PAddr, VAddr};

/// Which hart (core) is this?
#[inline]
pub fn r_mhartid() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {}, mhartid", out(reg) x);
    }
    x
}

bitflags! {
    /// Machine Status Register, mstatus.
    pub struct Mstatus: usize {
        /// Previous mode.
        const MPP_MASK = (3) << 11;
        const MPP_M = (3) << 11;
        const MPP_S = (1) << 11;
        const MPP_U = (0) << 11;
        /// Machine-mode interrupt enable.
        const MIE = (1) << 3;
    }
}

impl Mstatus {
    #[inline]
    pub fn read() -> Self {
        let mut x;
        unsafe {
            asm!("csrr {}, mstatus", out(reg) x);
        }
        Self::from_bits_truncate(x)
    }

    #[inline]
    pub unsafe fn write(self) {
        unsafe {
            asm!("csrw mstatus, {}", in(reg) self.bits());
        }
    }
}

/// Machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
pub unsafe fn w_mepc(x: usize) {
    unsafe {
        asm!("csrw mepc, {}", in(reg) x);
    }
}

bitflags! {
    /// Supervisor Status Register, sstatus.
    pub struct Sstatus: usize {
        /// Previous mode, 1=Supervisor, 0=User
        const SPP = (1) << 8;

        /// Supervisor Previous Interrupt Enable
        const SPIE = (1) << 5;

        /// User Previous Interrupt Enable
        const UPIE = (1) << 4;

        /// Supervisor Interrupt Enable
        const SIE = (1) << 1;

        /// User Interrupt Enable
        const UIE = (1) << 0;
    }

}

impl Sstatus {
    #[inline]
    pub fn read() -> Self {
        let mut x;
        unsafe {
            asm!("csrr {}, sstatus", out(reg) x);
        }
        Self::from_bits_truncate(x)
    }

    #[inline]
    pub unsafe fn write(self) {
        unsafe {
            asm!("csrw sstatus, {}", in(reg) self.bits());
        }
    }
}

/// Supervisor Interrupt Pending.
#[inline]
pub fn r_sip() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {}, sip", out(reg) x);
    }
    x
}
#[inline]
pub unsafe fn w_sip(x: usize) {
    unsafe {
        asm!("csrw sip, {}", in(reg) x);
    }
}

bitflags! {
    /// Supervisor Interrupt Enable.
    pub struct SIE: usize {
        /// external
        const SEIE = (1) << 9;

        /// timer
        const STIE = (1) << 5;

        /// software
        const SSIE = (1) << 1;

    }
}

impl SIE {
    #[inline]
    pub fn read() -> Self {
        let mut x;
        unsafe {
            asm!("csrr {}, sie", out(reg) x);
        }
        Self::from_bits_truncate(x)
    }

    #[inline]
    pub unsafe fn write(self) {
        unsafe {
            asm!("csrw sie, {}", in(reg) self.bits());
        }
    }
}

bitflags! {
    /// Machine-mode Interrupt Enable
    pub struct MIE: usize {
        /// external
        const MEIE = (1) << 11;

        /// timer
        const MTIE = (1) << 7;

        /// software
        const MSIE = (1) << 3;

        const ETC = !Self::MEIE.bits & !Self::MTIE.bits & !Self::MSIE.bits;
    }
}

impl MIE {
    #[inline]
    pub fn read() -> Self {
        let mut x: usize;
        unsafe {
            asm!("csrr {}, mie", out(reg) x);
        }
        Self::from_bits_truncate(x)
    }

    #[inline]
    pub unsafe fn write(self) {
        unsafe {
            asm!("csrw mie, {}", in(reg) self.bits());
        }
    }
}

/// Machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
pub unsafe fn w_sepc(x: usize) {
    unsafe {
        asm!("csrw sepc, {}", in(reg) x);
    }
}

#[inline]
pub fn r_sepc() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {}, sepc", out(reg) x);
    }
    x
}

/// Machine Exception Delegation.
#[inline]
pub fn r_medeleg() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {}, medeleg", out(reg) x);
    }
    x
}

#[inline]
pub unsafe fn w_medeleg(x: usize) {
    unsafe {
        asm!("csrw medeleg, {}", in(reg) x);
    }
}

/// Machine Interrupt Delegation.
#[inline]
pub fn r_mideleg() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {}, mideleg", out(reg) x);
    }
    x
}

#[inline]
pub unsafe fn w_mideleg(x: usize) {
    unsafe {
        asm!("csrw mideleg, {}", in(reg) x);
    }
}

/// Supervisor Trap-Vector Base Address
/// low two bits are mode.
#[inline]
pub unsafe fn w_stvec(x: usize) {
    unsafe {
        asm!("csrw stvec, {}", in(reg) x);
    }
}

#[inline]
pub fn r_stvec() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {}, stvec", out(reg) x);
    }
    x
}

/// Machine-mode interrupt vector.
#[inline]
pub unsafe fn w_mtvec(x: usize) {
    unsafe {
        asm!("csrw mtvec, {}", in(reg) x);
    }
}

/// Use riscv's sv39 page table scheme.
pub const SATP_SV39: usize = (8) << 60;

pub const fn make_satp(pagetable: usize) -> usize {
    SATP_SV39 | pagetable >> 12
}

/// Supervisor address translation and protection;
/// holds the address of the page table.
#[inline]
pub unsafe fn w_satp(x: usize) {
    unsafe {
        asm!("csrw satp, {}", in(reg) x);
    }
}

#[inline]
pub fn r_satp() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {}, satp", out(reg) x);
    }
    x
}

/// Supervisor Scratch register, for early trap handler in trampoline.S.
#[inline]
pub unsafe fn w_sscratch(x: usize) {
    unsafe {
        asm!("csrw sscratch, {}", in(reg) x);
    }
}

#[inline]
pub unsafe fn w_mscratch(x: usize) {
    unsafe {
        asm!("csrw mscratch, {}", in(reg) x);
    }
}

/// Supervisor Trap Cause.
#[inline]
pub fn r_scause() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {}, scause", out(reg) x);
    }
    x
}

/// Supervisor Trap Value.
#[inline]
pub fn r_stval() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {}, stval", out(reg) x);
    }
    x
}

/// Machine-mode Counter-Enable.
#[inline]
pub unsafe fn w_mcounteren(x: u64) {
    unsafe {
        asm!("csrw mcounteren, {}", in(reg) x);
    }
}

#[inline]
pub fn r_mcounteren() -> u64 {
    let mut x;
    unsafe {
        asm!("csrr {}, mcounteren", out(reg) x);
    }
    x
}

/// Machine-mode cycle counter.
#[inline]
pub fn r_time() -> u64 {
    let mut x;
    unsafe {
        asm!("csrr {}, time", out(reg) x);
    }
    x
}

/// Enable device interrupts.
#[inline]
pub unsafe fn intr_on() {
    let mut y = Sstatus::read();
    y.insert(Sstatus::SIE);
    unsafe { y.write() };
}

/// Disable device interrupts.
#[inline]
pub unsafe fn intr_off() {
    let mut x = Sstatus::read();
    x.remove(Sstatus::SIE);
    unsafe { x.write() };
}

/// Are device interrupts enabled?
#[inline]
pub fn intr_get() -> bool {
    Sstatus::read().contains(Sstatus::SIE)
}

/// Read and write tp, the thread pointer, which holds
/// this core's hartid (core number), the index into cpus[].
#[inline]
pub fn r_tp() -> usize {
    let mut x;
    unsafe {
        asm!("mv {}, tp", out(reg) x);
    }
    x
}

#[inline]
pub fn r_sp() -> usize {
    let mut x;
    unsafe {
        asm!("mv {}, sp", out(reg) x);
    }
    x
}

#[inline]
pub unsafe fn w_tp(x: usize) {
    unsafe {
        asm!("mv tp, {}", in(reg) x);
    }
}

#[inline]
pub fn r_ra() -> usize {
    let mut x;
    unsafe {
        asm!("mv {}, ra", out(reg) x);
    }
    x
}

/// Flush the TLB.
#[inline]
pub unsafe fn sfence_vma() {
    unsafe {
        // The zero, zero means flush all TLB entries.
        asm!("sfence.vma zero, zero");
    }
}

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

#[inline]
pub fn px<A: VAddr>(level: usize, va: A) -> usize {
    (va.into_usize() >> pxshift(level)) & PXMASK
}

/// One beyond the highest possible virtual address.
/// MAXVA is actually one bit less than the max allowed by
/// Sv39, to avoid having to sign-extend virtual addresses
/// that have the high bit set.
pub const MAXVA: usize = (1) << (9 + 9 + 9 + 12 - 1);
