//! the ARM Generic Interrupt Controller v3 (GIC v3).
// TODO: check correctness
use core::ptr;

use crate::arch::memlayout::GIC;

// pub const SGI_TYPE: usize = 		1;
// pub const PPI_TYPE: usize = 		2;
// pub const SPI_TYPE: usize = 		3;
// pub const GICD_CTLR: usize = 		0x000;
// pub const GICD_TYPER: usize = 		0x004;
// pub const GICD_IIDR: usize = 		0x008;
// pub const GICD_IGROUP: usize = 		0x080;
// pub const GICD_ISENABLE: usize = 		0x100;
// pub const GICD_ICENABLE: usize = 		0x180;
// pub const GICD_ISPEND: usize = 		0x200;
// pub const GICD_ICPEND: usize = 		0x280;
// pub const GICD_ISACTIVE: usize = 		0x300;
// pub const GICD_ICACTIVE: usize = 		0x380;
// pub const GICD_IPRIORITY: usize = 		0x400;
// pub const GICD_ITARGET: usize = 		0x800;
// pub const GICD_ICFG: usize = 		0xC00;

// pub const GICC_CTLR: usize = 		0x000;
// pub const GICC_PMR: usize = 		0x004;
// pub const GICC_BPR: usize = 		0x008;
// pub const GICC_IAR: usize = 		0x00C;
// pub const GICC_EOIR: usize = 		0x010;
// pub const GICC_RPR: usize = 		0x014;
// pub const GICC_HPPIR: usize = 		0x018;
// pub const GICC_ABPR: usize = 		0x01C;
// pub const GICC_AIAR: usize = 		0x020;
// pub const GICC_AEOIR: usize = 		0x024;
// pub const GICC_AHPPIR: usize = 		0x028;
// pub const GICC_APR: usize = 		0x0D0;
// pub const GICC_NSAPR: usize = 		0x0E0;
// pub const GICC_IIDR: usize = 		0x0FC;
// pub const GICC_DIR: usize = 		0x1000; /* v2 only */
// #[inline]
// fn gicd_reg(offset: usize) -> usize {
//     GIC.wrapping_add(offset)
// }

// #[inline]
// fn gicc_reg(offset: usize) -> usize {
//     GIC.wrapping_add(0x10000).wrapping_add(offset)
// }

// Distributor
const GICD_BASE: u64 = GIC as u64; // TODO: board-dependent value
const GICD_CTLR: *mut u32 = GICD_BASE as *mut u32;
const GICD_ISENABLER: *mut u32 = (GICD_BASE + 0x0100) as *mut u32;
// const GICD_ICENABLER: *mut u32 = (GICD_BASE + 0x0180) as *mut u32;
const GICD_ICPENDR: *mut u32 = (GICD_BASE + 0x0280) as *mut u32;
const GICD_ITARGETSR: *mut u32 = (GICD_BASE + 0x0800) as *mut u32;
const GICD_IPRIORITYR: *mut u32 = (GICD_BASE + 0x0400) as *mut u32;
const GICD_ICFGR: *mut u32 = (GICD_BASE + 0x0c00) as *mut u32;

const GICD_CTLR_ENABLE: u32 = 1;
// const GICD_CTLR_DISABLE: u32 = 0;
// const GICD_ICENABLER_SIZE: u32 = 32;
const GICD_ISENABLER_SIZE: u32 = 32;
const GICD_ICPENDR_SIZE: u32 = 32;
const GICD_ITARGETSR_SIZE: u32 = 4; // number of interrupts controlled by the register
const GICD_ITARGETSR_BITS: u32 = 8; // number of bits per interrupt

const GICD_IPRIORITY_SIZE: u32 = 4;
const GICD_IPRIORITY_BITS: u32 = 8;
const GICD_ICFGR_SIZE: u32 = 16;
const GICD_ICFGR_BITS: u32 = 2;

// CPU
const GICC_BASE: u64 = GICD_BASE + 0x10000; // TODO: board-dependent value
const GICC_CTLR: *mut u32 = GICC_BASE as *mut u32;
const GICC_PMR: *mut u32 = (GICC_BASE + 0x0004) as *mut u32;
const GICC_BPR: *mut u32 = (GICC_BASE + 0x0008) as *mut u32;
const GICC_CTLR_ENABLE: u32 = 1;
// const GICC_CTLR_DISABLE: u32 = 0;

const GICC_PMR_PRIO_LOW: u32 = 0xff;
// const GICC_PMR_PRIO_HIGH: u32 = 0x00;

const GICC_BPR_NO_GROUP: u32 = 0x00;

pub const ICFGR_EDGE: u32 = 2;

pub unsafe fn intr_init() {
    unsafe {
        ptr::write_volatile(GICD_CTLR, GICD_CTLR_ENABLE);
        ptr::write_volatile(GICC_CTLR, GICC_CTLR_ENABLE);
        ptr::write_volatile(GICC_PMR, GICC_PMR_PRIO_LOW);
        ptr::write_volatile(GICC_BPR, GICC_BPR_NO_GROUP);
    }
}

pub unsafe fn intr_init_hart() {
    todo!()
}

pub unsafe fn enable(interrupt: u32) {
    unsafe {
        ptr::write_volatile(
            GICD_ISENABLER.add((interrupt / GICD_ISENABLER_SIZE) as usize),
            1 << (interrupt % GICD_ISENABLER_SIZE),
        );
    }
}

// pub fn disable(interrupt: u32) {
//     unsafe {
//         ptr::write_volatile(
//             GICD_ICENABLER.add((interrupt / GICD_ICENABLER_SIZE) as usize),
//             1 << (interrupt % GICD_ICENABLER_SIZE)
//         );
//     }
// }

pub fn clear(interrupt: u32) {
    unsafe {
        ptr::write_volatile(
            GICD_ICPENDR.add((interrupt / GICD_ICPENDR_SIZE) as usize),
            1 << (interrupt % GICD_ICPENDR_SIZE),
        );
    }
}

pub fn set_core(interrupt: u32, core: u32) {
    let shift: u32 = (interrupt % GICD_ITARGETSR_SIZE) * GICD_ITARGETSR_BITS;
    unsafe {
        let addr: *mut u32 = GICD_ITARGETSR.add((interrupt / GICD_ITARGETSR_SIZE) as usize);
        let mut value: u32 = ptr::read_volatile(addr);
        value &= !(0xff << shift);
        value |= core << shift;
        ptr::write_volatile(addr, value);
    }
}

pub fn set_priority(interrupt: u32, priority: u32) {
    let shift = (interrupt % GICD_IPRIORITY_SIZE) * GICD_IPRIORITY_BITS;
    unsafe {
        let addr: *mut u32 = GICD_IPRIORITYR.add((interrupt / GICD_IPRIORITY_SIZE) as usize);
        let mut value: u32 = ptr::read_volatile(addr);
        value &= !(0xff << shift);
        value |= priority << shift;
        ptr::write_volatile(addr, value);
    }
}

pub fn set_config(interrupt: u32, config: u32) {
    let shift = (interrupt % GICD_ICFGR_SIZE) * GICD_ICFGR_BITS;
    unsafe {
        let addr: *mut u32 = GICD_ICFGR.add((interrupt / GICD_ICFGR_SIZE) as usize);
        let mut value: u32 = ptr::read_volatile(addr);
        value &= !(0x03 << shift);
        value |= config << shift;
        ptr::write_volatile(addr, value);
    }
}
