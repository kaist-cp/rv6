//! ARM instructions.

// Dead code is allowed in this file because not all components are used in the kernel.
#![allow(dead_code)]

/// Enable device interrupts.
#[inline]
pub unsafe fn intr_on() {
    unimplemented!()
}

/// Disable device interrupts.
#[inline]
pub fn intr_off() {
    unimplemented!()
}

/// Are device interrupts enabled?
#[inline]
pub fn intr_get() -> bool {
    unimplemented!()
}

/// Which hart (core) is this?
#[inline]
pub fn r_mpidr_el1() -> usize {
    let mut x:usize;
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
    unsafe {
        asm!("ic ialluis");
    }
}

/// flush TLB
pub fn tlbi_vmalle1() {
    unsafe {
        asm!("tlbi vmalle1")
    }
}

/// Instruction Synchronization Barrier.
pub fn isb() {
    unsafe {
        asm!("isb")
    }
}

/// Architectural Feature Access Control Register, EL1
pub unsafe fn w_cpacr_el1(x:usize) {
    unsafe {
        asm!("msr cpacr_el1, {}", in(reg) x)
    }
}

/// Monitor Debug System Control Register, EL1
pub unsafe fn w_mdscr_el1(x:usize) {
    unsafe {
        asm!("msr mdscr_el1, {}", in(reg) x)
    }
}
