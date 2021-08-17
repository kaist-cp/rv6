// //! the ARM Generic Interrupt Controller v3 (GIC v2).
// This code is from https://github.com/tonnylyz/rustpie/blob/master/src/driver/aarch64_virt/gic.rs

use cortex_a::registers::*;
use tock_registers::interfaces::{Readable, Writeable};
use tock_registers::{
    register_structs,
    registers::{ReadOnly, ReadWrite, WriteOnly},
};

use crate::arch::{
    asm::cpu_id,
    memlayout::{MemLayoutImpl, TIMER0_IRQ},
    timer::Timer,
};
use crate::memlayout::MemLayout;
use crate::timer::TimeManager;

const GIC_INTERRUPT_NUM: usize = 1024;
const GIC_SGI_NUM: usize = 16;
const GIC_1_BIT_NUM: usize = GIC_INTERRUPT_NUM / 32;
const GIC_8_BIT_NUM: usize = GIC_INTERRUPT_NUM * 8 / 32;
const GIC_2_BIT_NUM: usize = GIC_INTERRUPT_NUM * 2 / 32;

const GICD_BASE: usize = 0x08000000;
const GICC_BASE: usize = 0x08010000;

register_structs! {
  #[allow(non_snake_case)]
  GicDistributorBlock {
    (0x0000 => CTLR: ReadWrite<u32>),
    (0x0004 => TYPER: ReadOnly<u32>),
    (0x0008 => IIDR: ReadOnly<u32>),
    (0x000c => _reserved_0),
    (0x0080 => IGROUPR: [ReadWrite<u32>; GIC_1_BIT_NUM]),
    (0x0100 => ISENABLER: [ReadWrite<u32>; GIC_1_BIT_NUM]),
    (0x0180 => ICENABLER: [ReadWrite<u32>; GIC_1_BIT_NUM]),
    (0x0200 => ISPENDR: [ReadWrite<u32>; GIC_1_BIT_NUM]),
    (0x0280 => ICPENDR: [ReadWrite<u32>; GIC_1_BIT_NUM]),
    (0x0300 => ISACTIVER: [ReadWrite<u32>; GIC_1_BIT_NUM]),
    (0x0380 => ICACTIVER: [ReadWrite<u32>; GIC_1_BIT_NUM]),
    (0x0400 => IPRIORITYR: [ReadWrite<u32>; GIC_8_BIT_NUM]),
    (0x0800 => ITARGETSR: [ReadWrite<u32>; GIC_8_BIT_NUM]),
    (0x0c00 => ICFGR: [ReadWrite<u32>; GIC_2_BIT_NUM]),
    (0x0d00 => _reserved_1),
    (0x0e00 => NSACR: [ReadWrite<u32>; GIC_2_BIT_NUM]),
    (0x0f00 => SGIR: WriteOnly<u32>),
    (0x0f04 => _reserved_2),
    (0x0f10 => CPENDSGIR: [ReadWrite<u32>; GIC_SGI_NUM * 8 / 32]),
    (0x0f20 => SPENDSGIR: [ReadWrite<u32>; GIC_SGI_NUM * 8 / 32]),
    (0x0f30 => _reserved_3),
    (0x1000 => @END),
  }
}

struct GicDistributor {
    base_addr: usize,
}

impl core::ops::Deref for GicDistributor {
    type Target = GicDistributorBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr() }
    }
}

register_structs! {
  #[allow(non_snake_case)]
  GicCpuInterfaceBlock {
    (0x0000 => CTLR: ReadWrite<u32>),   // CPU Interface Control Register
    (0x0004 => PMR: ReadWrite<u32>),    // Interrupt Priority Mask Register
    (0x0008 => BPR: ReadWrite<u32>),    // Binary Point Register
    (0x000c => IAR: ReadOnly<u32>),     // Interrupt Acknowledge Register
    (0x0010 => EOIR: WriteOnly<u32>),   // End of Interrupt Register
    (0x0014 => RPR: ReadOnly<u32>),     // Running Priority Register
    (0x0018 => HPPIR: ReadOnly<u32>),   // Highest Priority Pending Interrupt Register
    (0x001c => ABPR: ReadWrite<u32>),   // Aliased Binary Point Register
    (0x0020 => AIAR: ReadOnly<u32>),    // Aliased Interrupt Acknowledge Register
    (0x0024 => AEOIR: WriteOnly<u32>),  // Aliased End of Interrupt Register
    (0x0028 => AHPPIR: ReadOnly<u32>),  // Aliased Highest Priority Pending Interrupt Register
    (0x002c => _reserved_0),
    (0x00d0 => APR: [ReadWrite<u32>; 4]),    // Active Priorities Register
    (0x00e0 => NSAPR: [ReadWrite<u32>; 4]),  // Non-secure Active Priorities Register
    (0x00f0 => _reserved_1),
    (0x00fc => IIDR: ReadOnly<u32>),    // CPU Interface Identification Register
    (0x0100 => _reserved_2),
    (0x1000 => DIR: WriteOnly<u32>),    // Deactivate Interrupt Register
    (0x1004 => _reserved_3),
    (0x2000 => @END),
  }
}

struct GicCpuInterface {
    base_addr: usize,
}

impl core::ops::Deref for GicCpuInterface {
    type Target = GicCpuInterfaceBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr() }
    }
}

impl GicCpuInterface {
    const fn new(base_addr: usize) -> Self {
        GicCpuInterface { base_addr }
    }

    fn ptr(&self) -> *const GicCpuInterfaceBlock {
        self.base_addr as *const _
    }

    fn init(&self) {
        self.PMR.set(u32::MAX);
        self.CTLR.set(1);
    }
}

impl GicDistributor {
    const fn new(base_addr: usize) -> Self {
        GicDistributor { base_addr }
    }

    fn ptr(&self) -> *const GicDistributorBlock {
        self.base_addr as *const _
    }

    fn init(&self) {
        let max_spi = (self.TYPER.get() & 0b11111) * 32 + 1;
        for i in 1usize..(max_spi as usize / 32) {
            self.ICENABLER[i].set(u32::MAX);
            self.ICPENDR[i].set(u32::MAX);
            self.ICACTIVER[i].set(u32::MAX);
        }
        for i in 8usize..(max_spi as usize * 8 / 32) {
            self.IPRIORITYR[i].set(u32::MAX);
            self.ITARGETSR[i].set(u32::MAX);
        }
        self.CTLR.set(1);
    }

    fn init_per_core(&self) {
        self.ICENABLER[0].set(u32::MAX);
        self.ICPENDR[0].set(u32::MAX);
        self.ICACTIVER[0].set(u32::MAX);
        for i in 0..4 {
            self.CPENDSGIR[i].set(u32::MAX);
        }
        for i in 0..8 {
            self.IPRIORITYR[i].set(u32::MAX);
        }
    }

    fn set_enable(&self, int: usize) {
        let idx = int / 32;
        let bit = 1u32 << (int % 32);
        self.ISENABLER[idx].set(bit);
    }

    fn clear_enable(&self, int: usize) {
        let idx = int / 32;
        let bit = 1u32 << (int % 32);
        self.ICENABLER[idx].set(bit);
    }

    fn set_target(&self, int: usize, target: u8) {
        let idx = (int * 8) / 32;
        let offset = (int * 8) % 32;
        let mask: u32 = 0b11111111 << offset;
        let prev = self.ITARGETSR[idx].get();
        self.ITARGETSR[idx].set((prev & (!mask)) | (((target as u32) << offset) & mask));
    }

    fn set_priority(&self, int: usize, priority: u8) {
        let idx = (int * 8) / 32;
        let offset = (int * 8) % 32;
        let mask: u32 = 0b11111111 << offset;
        let prev = self.IPRIORITYR[idx].get();
        self.IPRIORITYR[idx].set((prev & (!mask)) | (((priority as u32) << offset) & mask));
    }

    fn set_config(&self, int: usize, edge: bool) {
        let idx = (int * 2) / 32;
        let offset = (int * 2) % 32;
        let mask: u32 = 0b11 << offset;
        let prev = self.ICFGR[idx].get();
        self.ICFGR[idx].set((prev & (!mask)) | ((if edge { 0b10 } else { 0b00 } << offset) & mask));
    }
}

static GICD: GicDistributor = GicDistributor::new(GICD_BASE);
static GICC: GicCpuInterface = GicCpuInterface::new(GICC_BASE);

#[derive(Debug)]
pub struct Gic;

impl Gic {
    pub fn init(&self) {
        let core_id = cpu_id();
        let gicd = &GICD;
        if core_id == 0 {
            gicd.init();
        }
        let gicc = &GICC;
        gicd.init_per_core();
        gicc.init();
    }

    pub fn enable(&self, int: Interrupt) {
        let core_id = cpu_id();
        let gicd = &GICD;
        gicd.set_enable(int);
        gicd.set_priority(int, 0x7f);
        if int >= 32 {
            gicd.set_config(int, true);
        }
        gicd.set_target(int, (1 << core_id) as u8);
    }

    pub fn disable(&self, int: Interrupt) {
        let gicd = &GICD;
        gicd.clear_enable(int);
    }

    pub fn fetch(&self) -> Option<Interrupt> {
        let gicc = &GICC;
        let i = gicc.IAR.get();
        if i >= 1022 {
            None
        } else {
            Some(i as Interrupt)
        }
    }

    pub fn finish(&self, int: Interrupt) {
        let gicc = &GICC;
        gicc.EOIR.set(int as u32);
    }
}

pub const INT_TIMER: Interrupt = 27; // virtual timer

pub static INTERRUPT_CONTROLLER: Gic = Gic {};

pub type Interrupt = usize;

pub unsafe fn intr_init() {}

pub unsafe fn intr_init_core() {
    DAIF.set(DAIF::I::Masked.into());
    INTERRUPT_CONTROLLER.init();

    INTERRUPT_CONTROLLER.enable(TIMER0_IRQ);
    Timer::init();

    // Order matters!
    if cpu_id() == 0 {
        // only boot core do this initialization

        // virtio_blk
        INTERRUPT_CONTROLLER.enable(MemLayoutImpl::VIRTIO0_IRQ);

        // pl011 uart
        INTERRUPT_CONTROLLER.enable(MemLayoutImpl::UART0_IRQ);
    }
}
