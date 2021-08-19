//! ARM instructions.

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

const DIS_INT: usize = 0x80;

/// Enable device interrupts (IRQ).
///
/// # Safety
///
/// Interrupt handlers must be set properly.
#[inline]
pub unsafe fn intr_on() {
    unsafe {
        asm!("msr daifclr, #2");
    }
}

/// Disable device interrupts (IRQ).
#[inline]
pub fn intr_off() {
    // SAFETY: turning interrupt off is safe.
    unsafe {
        asm!("msr daifset, #2");
    }
}

/// Are device interrupts (IRQ) enabled?
#[inline]
pub fn intr_get() -> bool {
    let mut x: usize;
    unsafe {
        asm!("mrs {}, daif", out(reg) x);
    }
    x & DIS_INT == 0
}

/// Which hart (core) is this?
#[inline]
pub fn cpu_id() -> usize {
    let mut x: usize;
    unsafe {
        asm!("mrs {}, mpidr_el1", out(reg) x);
    }
    x & 0b11
}

/// get current EL
pub fn r_currentel() -> usize {
    let mut x: usize;
    unsafe {
        asm!("mrs {}, CurrentEL", out(reg) x);
    }
    (x & 0x0c) >> 2
}

/// read the main id register
pub fn r_midr_el1() -> usize {
    let mut x: usize;
    unsafe {
        asm!("mrs {}, midr_el1", out(reg) x);
    }
    x
}

/// flush instruction cache
pub fn ic_ialluis() {
    unsafe { asm!("ic ialluis") }
}

/// flush TLB
pub fn tlbi_vmalle1() {
    unsafe { asm!("tlbi vmalle1") }
}

/// Instruction Synchronization Barrier.
pub fn isb() {
    unsafe { asm!("isb") }
}

/// Write to Architectural Feature Access Control Register, EL1
///
/// # Safety
///
/// `x` must contain valid value for cpacr_el1 register.
pub unsafe fn w_cpacr_el1(x: usize) {
    unsafe { asm!("msr cpacr_el1, {}", in(reg) x) }
}

/// Write to Monitor Debug System Control Register, EL1
///
/// # Safety
///
/// `x` must contain valid value for mdscr_el1 register.
pub unsafe fn w_mdscr_el1(x: usize) {
    if x == 0 {
        unsafe { asm!("msr mdscr_el1, xzr") }
    }
    unsafe { asm!("msr mdscr_el1, {}", in(reg) x) }
}

pub fn r_fpsr() -> usize {
    let mut x;
    unsafe { asm! ("mrs {}, fpsr", out(reg) x) };
    x
}

/// Write to Floating-point Status Register
///
/// # Safety
///
/// `x` must contain valid value for mdscr_el1 register.
pub unsafe fn w_fpsr(x: usize) {
    unsafe { asm!("msr fpsr, {}", in(reg) x) }
}

#[derive(Debug)]
pub enum SmcFunctions {
    _Version = 0x84000000,
    _SuspendAarch64 = 0xc4000001,
    _CpuOff = 0x84000002,
    CpuOn = 0xc4000003,
    _AffinityInfoAarch64 = 0xc4000004,
    _Features = 0x8400000A,
    _MigInfoType = 0x84000006,
    _SystemOff = 0x84000008,
    _SystemReset = 0x84000009,
}

/// Secure Monitor call
///
/// # Safety
///
/// Arguments must follow ARM SMC calling convention.
#[no_mangle]
pub unsafe fn smc_call(x0: u64, x1: u64, x2: u64, x3: u64) -> u64 {
    let r;
    unsafe {
        // NOTE: here use hvc for qemu without `virtualization=on`
        asm!("hvc #0", inlateout("x0") x0 => r, in("x1") x1, in("x2") x2, in("x3") x3);
    }
    r
}

pub fn cpu_relax() {
    barrier();
}

pub fn r_mpidr() -> usize {
    let mut x: usize;
    unsafe {
        asm!("mrs {}, mpidr_el1", out(reg) x);
    }
    x
}

pub fn r_icc_ctlr_el1() -> u32 {
    let mut x: usize;
    unsafe { asm!("mrs {}, icc_ctlr_el1", out(reg) x) };
    x as u32
}

pub fn barrier() {
    unsafe {
        asm!("isb sy");
        asm!("dsb sy");
        asm!("dsb ishst");
        asm!("tlbi vmalle1is");
        asm!("dsb ish");
        asm!("isb");
    }
}
