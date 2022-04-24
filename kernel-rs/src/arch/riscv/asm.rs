//! RISC-V instructions.
// TODO(https://github.com/kaist-cp/rv6/issues/569): replace this

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

use core::arch::asm;

use bitflags::bitflags;

/// Which hart (core) is this?
#[inline]
pub fn r_mhartid() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {x}, mhartid", x = out(reg) x);
    }
    x
}

pub fn cpu_id() -> usize {
    r_tp()
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
            asm!("csrr {x}, mstatus", x = out(reg) x);
        }
        Self::from_bits_truncate(x)
    }

    #[inline]
    pub unsafe fn write(self) {
        unsafe {
            asm!("csrw mstatus, {x}", x = in(reg) self.bits());
        }
    }
}

/// Machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
pub unsafe fn w_mepc(x: usize) {
    unsafe {
        asm!("csrw mepc, {x}", x = in(reg) x);
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
            asm!("csrr {x}, sstatus", x = out(reg) x);
        }
        Self::from_bits_truncate(x)
    }

    #[inline]
    pub unsafe fn write(self) {
        unsafe {
            asm!("csrw sstatus, {x}", x = in(reg) self.bits());
        }
    }
}

/// Supervisor Interrupt Pending.
#[inline]
pub fn r_sip() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {x}, sip", x = out(reg) x);
    }
    x
}
#[inline]
pub unsafe fn w_sip(x: usize) {
    unsafe {
        asm!("csrw sip, {x}", x = in(reg) x);
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
            asm!("csrr {x}, sie", x = out(reg) x);
        }
        Self::from_bits_truncate(x)
    }

    #[inline]
    pub unsafe fn write(self) {
        unsafe {
            asm!("csrw sie, {x}", x = in(reg) self.bits());
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
            asm!("csrr {x}, mie", x = out(reg) x);
        }
        Self::from_bits_truncate(x)
    }

    #[inline]
    pub unsafe fn write(self) {
        unsafe {
            asm!("csrw mie, {x}", x = in(reg) self.bits());
        }
    }
}

/// Machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
pub unsafe fn w_sepc(x: usize) {
    unsafe {
        asm!("csrw sepc, {x}", x = in(reg) x);
    }
}

#[inline]
pub fn r_sepc() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {x}, sepc", x = out(reg) x);
    }
    x
}

/// Machine Exception Delegation.
#[inline]
pub fn r_medeleg() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {x}, medeleg", x = out(reg) x);
    }
    x
}

#[inline]
pub unsafe fn w_medeleg(x: usize) {
    unsafe {
        asm!("csrw medeleg, {x}", x = in(reg) x);
    }
}

/// Machine Interrupt Delegation.
#[inline]
pub fn r_mideleg() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {x}, mideleg", x = out(reg) x);
    }
    x
}

#[inline]
pub unsafe fn w_mideleg(x: usize) {
    unsafe {
        asm!("csrw mideleg, {x}", x = in(reg) x);
    }
}

/// Supervisor Trap-Vector Base Address
/// low two bits are mode.
#[inline]
pub unsafe fn w_stvec(x: usize) {
    unsafe {
        asm!("csrw stvec, {x}", x = in(reg) x);
    }
}

#[inline]
pub fn r_stvec() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {x}, stvec", x = out(reg) x);
    }
    x
}

/// Machine-mode interrupt vector.
#[inline]
pub unsafe fn w_mtvec(x: usize) {
    unsafe {
        asm!("csrw mtvec, {x}", x = in(reg) x);
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
        asm!("csrw satp, {x}", x = in(reg) x);
    }
}

#[inline]
pub fn r_satp() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {x}, satp", x = out(reg) x);
    }
    x
}

/// Supervisor Scratch register, for early trap handler in trampoline.S.
#[inline]
pub unsafe fn w_sscratch(x: usize) {
    unsafe {
        asm!("csrw sscratch, {x}", x = in(reg) x);
    }
}

#[inline]
pub unsafe fn w_mscratch(x: usize) {
    unsafe {
        asm!("csrw mscratch, {x}", x = in(reg) x);
    }
}

/// Supervisor Trap Cause.
#[inline]
pub fn r_scause() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {x}, scause", x = out(reg) x);
    }
    x
}

/// Supervisor Trap Value.
#[inline]
pub fn r_stval() -> usize {
    let mut x;
    unsafe {
        asm!("csrr {x}, stval", x = out(reg) x);
    }
    x
}

/// Machine-mode Counter-Enable.
#[inline]
pub unsafe fn w_mcounteren(x: u64) {
    unsafe {
        asm!("csrw mcounteren, {x}", x = in(reg) x);
    }
}

#[inline]
pub fn r_mcounteren() -> u64 {
    let mut x;
    unsafe {
        asm!("csrr {x}, mcounteren", x = out(reg) x);
    }
    x
}

/// Machine-mode cycle counter.
#[inline]
pub fn r_time() -> u64 {
    let mut x;
    unsafe {
        asm!("csrr {x}, time", x = out(reg) x);
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
pub fn intr_off() {
    let mut x = Sstatus::read();
    x.remove(Sstatus::SIE);
    // SAFETY: turning interrupt off is safe.
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
        asm!("mv {x}, tp", x = out(reg) x);
    }
    x
}

#[inline]
pub fn r_sp() -> usize {
    let mut x;
    unsafe {
        asm!("mv {x}, sp", x = out(reg) x);
    }
    x
}

#[inline]
pub unsafe fn w_tp(x: usize) {
    unsafe {
        asm!("mv tp, {x}", x = in(reg) x);
    }
}

#[inline]
pub fn r_ra() -> usize {
    let mut x;
    unsafe {
        asm!("mv {x}, ra", x = out(reg) x);
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
