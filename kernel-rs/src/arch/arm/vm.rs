use core::pin::Pin;

use bitflags::bitflags;
use cortex_a::registers::*;
use tock_registers::interfaces::ReadWriteable;

use crate::{
    addr::{PAddr, VAddr, PGSIZE},
    arch::{
        addr::{pa2pte, pte2pa},
        asm::{isb, tlbi_vmalle1},
        memlayout::{MemLayoutImpl, GIC},
    },
    kalloc::Kmem,
    lock::SpinLock,
    memlayout::MemLayout,
    vm::{AccessFlags, PageInitiator, PageTable, PageTableEntryDesc, RawPageTable},
};

extern "C" {
    // kernel.ld sets this to end of kernel code.
    static mut etext: [u8; 0];

    // trampoline.S
    static mut trampoline: [u8; 0];
}

// A table descriptor and a level 3 page descriptor as per
// ARMv8-A Architecture Reference Manual Figure D5-15, and Figure D5-17 respectively.
bitflags! {
    pub struct ArmV8PteFlags: usize {
        /// valid
        const V = 1 << 0;
        const TABLE = 1 << 1; // !table = block
        const PAGE = 1 << 1;
        /// Non-Secure Bit: always non-secure now
        const NON_SECURE_PA = 1 << 5;
        /// AP flags
        const RW_P = 0 << 6;
        const RW_U = 1 << 6; // EL0, 1 both can access
        const RO_P = 2 << 6;
        const RO_U = 3 << 6;
        const U = 1 << 6;
        /// Access Flag
        const ACCESS_FLAG = 1 << 10;
        /// Unprivileged execute-never, stage 1 only
        const UXN = 1 << 54;
        /// Privileged execute-never, stage 1 only
        const PXN = 1 << 53;

        // TODO: are these necessary?
        const MEM_ATTR_IDX_0 = (0 << 2);
        const MEM_ATTR_IDX_1 = (1 << 2);
        const MEM_ATTR_IDX_2 = (2 << 2);
        const MEM_ATTR_IDX_3 = (3 << 2);
        const MEM_ATTR_IDX_4 = (4 << 2);
        const MEM_ATTR_IDX_5 = (5 << 2);
        const MEM_ATTR_IDX_6 = (6 << 2);
        const MEM_ATTR_IDX_7 = (7 << 2);
    }
}

pub type PteFlags = ArmV8PteFlags;

impl From<AccessFlags> for ArmV8PteFlags {
    fn from(item: AccessFlags) -> Self {
        let mut ret = Self::empty();
        if item.intersects(AccessFlags::R) {
            ret |= Self::ACCESS_FLAG;
            if item.intersects(AccessFlags::W) {
                ret |= Self::RW_P;
            } else {
                ret |= Self::RO_P;
            }
        }
        if item.intersects(AccessFlags::X) {
            if !item.intersects(AccessFlags::U) {
                ret |= Self::UXN;
            }
        } else {
            ret |= Self::UXN | Self::PXN;
        }
        if item.intersects(AccessFlags::U) {
            ret |= Self::U;
        }
        ret
    }
}

/// # Safety
///
/// If self.is_table() is true, then it must refer to a valid page-table page.
///
/// Because of #[derive(Default)], inner is initially 0, which satisfies the invariant.
#[derive(Default)]
pub struct ArmV8PageTableEntry {
    inner: usize,
}

pub type PageTableEntry = ArmV8PageTableEntry;

impl PageTableEntryDesc for ArmV8PageTableEntry {
    type EntryFlags = ArmV8PteFlags;

    fn get_flags(&self) -> Self::EntryFlags {
        Self::EntryFlags::from_bits_truncate(self.inner)
    }

    fn flag_intersects(&self, flag: Self::EntryFlags) -> bool {
        self.get_flags().intersects(flag)
    }

    fn get_pa(&self) -> PAddr {
        pte2pa(self.inner)
    }

    fn is_valid(&self) -> bool {
        self.flag_intersects(Self::EntryFlags::V)
    }

    fn is_user(&self) -> bool {
        self.flag_intersects(Self::EntryFlags::V | Self::EntryFlags::U)
    }

    fn is_table(&self) -> bool {
        self.is_valid()
            && self.flag_intersects(Self::EntryFlags::TABLE)
            && !self.flag_intersects(Self::EntryFlags::ACCESS_FLAG)
    }

    fn is_data(&self) -> bool {
        self.is_valid()
            && self.flag_intersects(Self::EntryFlags::PAGE | Self::EntryFlags::ACCESS_FLAG)
    }

    /// Make the entry refer to a given page-table page.
    fn set_table(&mut self, page: *mut RawPageTable) {
        self.inner = pa2pte((page as usize).into())
            | Self::EntryFlags::V.bits()
            | Self::EntryFlags::TABLE.bits();
    }

    /// Make the entry refer to a given address with a given permission.
    /// The permission should include at lease one of R, W, and X not to be
    /// considered as an entry referring a page-table page.
    fn set_entry(&mut self, pa: PAddr, perm: Self::EntryFlags) {
        // assert!(perm.intersects(Self::EntryFlags::R | Self::EntryFlags::W | Self::EntryFlags::X));
        self.inner = pa2pte(pa)
            | (perm
                | Self::EntryFlags::V
                | Self::EntryFlags::NON_SECURE_PA
                | Self::EntryFlags::ACCESS_FLAG
                | Self::EntryFlags::PAGE)
                .bits();
    }

    /// Make the entry inaccessible by user processes by clearing Self::EntryFlags::U.
    fn clear_user(&mut self) {
        self.inner &= !(Self::EntryFlags::U.bits());
    }

    /// Invalidate the entry by making every bit 0.
    fn invalidate(&mut self) {
        self.inner = 0;
    }
}

pub struct RiscVPageInit;

pub type PageInit = RiscVPageInit;

impl PageInitiator for RiscVPageInit {
    fn user_page_init<A: VAddr>(
        page_table: &mut PageTable<A>,
        trap_frame: PAddr,
        allocator: Pin<&SpinLock<Kmem>>,
    ) -> Result<(), ()> {
        // Map the trampoline code (for system call return)
        // at the highest user virtual address.
        // Only the supervisor uses it, on the way
        // to/from user space, so not PTE_U.
        page_table.insert(
            MemLayoutImpl::TRAMPOLINE.into(),
            // SAFETY: we assume that reading the address of trampoline is safe.
            (unsafe { trampoline.as_mut_ptr() as usize }).into(),
            ArmV8PteFlags::RO_P | ArmV8PteFlags::UXN,
            allocator,
        )?;

        // Map the trapframe just below TRAMPOLINE, for trampoline.S.
        page_table.insert(
            MemLayoutImpl::TRAPFRAME.into(),
            trap_frame,
            ArmV8PteFlags::RW_P | ArmV8PteFlags::PXN | ArmV8PteFlags::UXN,
            allocator,
        )?;

        Ok(())
    }

    fn kernel_page_init<A: VAddr>(
        page_table: &mut PageTable<A>,
        allocator: Pin<&SpinLock<Kmem>>,
    ) -> Result<(), ()> {
        // SiFive Test Finisher MMIO
        // page_table
        //     .insert_range(
        //         FINISHER.into(),
        //         PGSIZE,
        //         FINISHER.into(),
        //         PteFlags::R | PteFlags::W,
        //         allocator,
        //     )
        //     .ok()?;

        // Uart registers
        page_table.insert_range(
            MemLayoutImpl::UART0.into(),
            PGSIZE,
            MemLayoutImpl::UART0.into(),
            ArmV8PteFlags::RW_P | ArmV8PteFlags::PXN,
            allocator,
        )?;

        // Virtio mmio disk interface
        page_table.insert_range(
            MemLayoutImpl::VIRTIO0.into(),
            PGSIZE,
            MemLayoutImpl::VIRTIO0.into(),
            ArmV8PteFlags::RW_P | ArmV8PteFlags::PXN,
            allocator,
        )?;

        // GIC
        page_table.insert_range(
            GIC.into(),
            MemLayoutImpl::UART0 - GIC,
            GIC.into(),
            ArmV8PteFlags::RW_P | ArmV8PteFlags::PXN,
            allocator,
        )?;

        // Map the trampoline for trap entry/exit to
        // the highest virtual address in the kernel.
        page_table.insert_range(
            MemLayoutImpl::TRAMPOLINE.into(),
            PGSIZE,
            // SAFETY: we assume that reading the address of trampoline is safe.
            unsafe { trampoline.as_mut_ptr() as usize }.into(),
            ArmV8PteFlags::RO_P | ArmV8PteFlags::UXN,
            allocator,
        )?;

        Ok(())
    }

    unsafe fn switch_page_table_and_enable_mmu(page_table_base: usize) {
        // We don't use upper VA space
        // TTBR1_EL1.set_baddr(page_table_base as u64);

        isb();
        // register page table
        TTBR0_EL1.set_baddr(page_table_base as u64);

        // Enable MMU.
        SCTLR_EL1.modify(SCTLR_EL1::M::Enable + SCTLR_EL1::C::Cacheable + SCTLR_EL1::I::Cacheable);

        // Force MMU init to complete before next instruction.
        isb();
        tlbi_vmalle1();
    }
}
