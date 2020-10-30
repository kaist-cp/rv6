use crate::vm::{PAddr, VAddr};

/// Which hart (core) is this?
#[inline]
pub unsafe fn r_mhartid() -> usize {
    let mut x;
    llvm_asm!("csrr $0, mhartid" : "=r" (x) : : : "volatile");
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
    pub unsafe fn read() -> Self {
        let mut x;
        llvm_asm!("csrr $0, mstatus" : "=r" (x) : : : "volatile");
        x
    }
    #[inline]
    pub unsafe fn write(self) {
        llvm_asm!("csrw mstatus, $0" : : "r" (self) : : "volatile");
    }
}

/// Machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
pub unsafe fn w_mepc(x: usize) {
    llvm_asm!("csrw mepc, $0" : : "r" (x) : : "volatile");
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
    pub unsafe fn read() -> Self {
        let mut x;
        llvm_asm!("csrr $0, sstatus" : "=r" (x) : : : "volatile");
        x
    }
    #[inline]
    pub unsafe fn write(self) {
        llvm_asm!("csrw sstatus, $0" : : "r" (self) : : "volatile");
    }
}

/// Supervisor Interrupt Pending.
#[inline]
pub unsafe fn r_sip() -> usize {
    let mut x;
    llvm_asm!("csrr $0, sip" : "=r" (x) : : : "volatile");
    x
}
#[inline]
pub unsafe fn w_sip(x: usize) {
    llvm_asm!("csrw sip, $0" : : "r" (x) : : "volatile");
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
    pub unsafe fn read() -> Self {
        let mut x;
        llvm_asm!("csrr $0, sie" : "=r" (x) : : : "volatile");
        x
    }

    #[inline]
    pub unsafe fn write(self) {
        llvm_asm!("csrw sie, $0" : : "r" (self) : : "volatile");
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
    }
}

impl MIE {
    #[inline]
    pub unsafe fn read() -> Self {
        let mut x;
        llvm_asm!("csrr $0, mie" : "=r" (x) : : : "volatile");
        x
    }

    #[inline]
    pub unsafe fn write(self) {
        llvm_asm!("csrw mie, $0" : : "r" (self) : : "volatile");
    }
}

/// Machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
pub unsafe fn w_sepc(x: usize) {
    llvm_asm!("csrw sepc, $0" : : "r" (x) : : "volatile");
}

#[inline]
pub unsafe fn r_sepc() -> usize {
    let mut x;
    llvm_asm!("csrr $0, sepc" : "=r" (x) : : : "volatile");
    x
}

/// Machine Exception Delegation.
#[inline]
pub unsafe fn r_medeleg() -> usize {
    let mut x;
    llvm_asm!("csrr %0, medeleg" : "=r" (x) : : : "volatile");
    x
}

#[inline]
pub unsafe fn w_medeleg(x: usize) {
    llvm_asm!("csrw medeleg, $0" : : "r" (x) : : "volatile");
}

/// Machine Interrupt Delegation.
#[inline]
pub unsafe fn r_mideleg() -> usize {
    let mut x;
    llvm_asm!("csrr %0, mideleg" : "=r" (x) : : : "volatile");
    x
}

#[inline]
pub unsafe fn w_mideleg(x: usize) {
    llvm_asm!("csrw mideleg, $0" : : "r" (x) : : "volatile");
}

/// Supervisor Trap-Vector Base Address
/// low two bits are mode.
#[inline]
pub unsafe fn w_stvec(x: usize) {
    llvm_asm!("csrw stvec, $0" : : "r" (x) : : "volatile");
}

#[inline]
pub unsafe fn r_stvec() -> usize {
    let mut x;
    llvm_asm!("csrr %0, stvec" : "=r" (x) : : : "volatile");
    x
}

/// Machine-mode interrupt vector.
#[inline]
pub unsafe fn w_mtvec(x: usize) {
    llvm_asm!("csrw mtvec, $0" : : "r" (x) : : "volatile");
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
    llvm_asm!("csrw satp, $0" : : "r" (x) : : "volatile");
}

#[inline]
pub unsafe fn r_satp() -> usize {
    let mut x;
    llvm_asm!("csrr $0, satp" : "=r" (x) : : : "volatile");
    x
}

/// Supervisor Scratch register, for early trap handler in trampoline.S.
#[inline]
pub unsafe fn w_sscratch(x: usize) {
    llvm_asm!("csrw sscratch, %0" : : "r" (x) : : : "volatile");
}

#[inline]
pub unsafe fn w_mscratch(x: usize) {
    llvm_asm!("csrw mscratch, $0" : : "r" (x) : : "volatile");
}

/// Supervisor Trap Cause.
#[inline]
pub unsafe fn r_scause() -> usize {
    let mut x;
    llvm_asm!("csrr $0, scause" : "=r" (x) : : : "volatile");
    x
}

/// Supervisor Trap Value.
#[inline]
pub unsafe fn r_stval() -> usize {
    let mut x;
    llvm_asm!("csrr $0, stval" : "=r" (x) : : : "volatile");
    x
}

/// Machine-mode Counter-Enable.
#[inline]
pub unsafe fn w_mcounteren(x: u64) {
    llvm_asm!("csrw mcounteren, %0" : : "r" (x)  : : : "volatile");
}

#[inline]
pub unsafe fn r_mcounteren() -> u64 {
    let mut x;
    llvm_asm!("csrr %0, mcounteren" : "=r" (x) : : : "volatile");
    x
}

/// Machine-mode cycle counter.
#[inline]
pub unsafe fn r_time() -> u64 {
    let mut x;
    llvm_asm!("csrr %0, time" : "=r" (x) : : : "volatile");
    x
}

/// Enable device interrupts.
#[inline]
pub unsafe fn intr_on() {
    let mut x = SIE::read();
    x.insert(SIE::SEIE);
    x.insert(SIE::STIE);
    x.insert(SIE::SSIE);
    x.write();
    let mut y = Sstatus::read();
    y.insert(Sstatus::SIE);
    y.write();
}

/// Disable device interrupts.
#[inline]
pub unsafe fn intr_off() {
    let mut x = Sstatus::read();
    x.remove(Sstatus::SIE);
    x.write();
}

/// Are device interrupts enabled?
#[inline]
pub unsafe fn intr_get() -> bool {
    Sstatus::read().contains(Sstatus::SIE)
}

/// Read and write tp, the thread pointer, which holds
/// this core's hartid (core number), the index into cpus[].
#[inline]
pub unsafe fn r_tp() -> usize {
    let mut x;
    llvm_asm!("mv $0, tp" : "=r" (x) : : : "volatile");
    x
}

#[inline]
pub unsafe fn r_sp() -> usize {
    let mut x;
    llvm_asm!("mv %0, sp" : "=r" (x) : : : "volatile");
    x
}

#[inline]
pub unsafe fn w_tp(x: usize) {
    llvm_asm!("mv tp, $0" : : "r" (x) : : "volatile");
}

#[inline]
pub unsafe fn r_ra() -> usize {
    let mut x;
    llvm_asm!("mv %0, ra" : "=r" (x) : : : "volatile");
    x
}

/// Flush the TLB.
#[inline]
pub unsafe fn sfence_vma() {
    // The zero, zero means flush all TLB entries.
    llvm_asm!("sfence.vma zero, zero" : : : : "volatile");
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

/// valid
pub const PTE_V: usize = (1) << 0;

pub const PTE_R: i32 = (1) << 1;
pub const PTE_W: i32 = (1) << 2;
pub const PTE_X: i32 = (1) << 3;

/// 1 -> user can access
pub const PTE_U: i32 = (1) << 4;

/// Shift a physical address to the right place for a PTE.
#[inline]
pub const fn pa2pte(pa: PAddr) -> usize {
    (pa.into_usize() >> 12) << 10
}

#[inline]
pub const fn pte2pa(pte: PteT) -> PAddr {
    PAddr::new((pte >> 10) << 12)
}

#[inline]
pub const fn pte_flags(pte: PteT) -> usize {
    pte & 0x3FFusize
}

/// Extract the three 9-bit page table indices from a virtual address.

/// 9 bits
pub const PXMASK: usize = 0x1ff;

#[inline]
fn pxshift(level: usize) -> usize {
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

pub type PteT = usize;
