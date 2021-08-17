use core::pin::Pin;

use bitflags::bitflags;

use crate::{
<<<<<<< HEAD
    addr::{pa2pte, pte2pa, PAddr, VAddr, PGSIZE},
    arch::asm::{make_satp, sfence_vma, w_satp},
    arch::memlayout::{
        kstack, FINISHER, KERNBASE, PHYSTOP, PLIC, TRAMPOLINE, TRAPFRAME, UART0, VIRTIO0,
    },
    arch::riscv::{make_satp, sfence_vma, w_satp},
    fs::{DefaultFs, InodeGuard},
    arch::asm::{make_satp, sfence_vma, w_satp},
    fs::{FileSystem, InodeGuard, Ufs},
    arch::memlayout::{FINISHER, PLIC, TRAMPOLINE, TRAPFRAME, UART0, VIRTIO0},
    arch::memlayout::MemLayoutImpl,
    arch::memlayout::{FINISHER, PLIC},
=======
    addr::{PAddr, VAddr, PGSIZE},
    arch::{
        addr::{pa2pte, pte2pa},
        asm::{make_satp, sfence_vma, w_satp},
        memlayout::{MemLayout, FINISHER, PLIC},
    },
>>>>>>> refactoring
    kalloc::Kmem,
    lock::SpinLock,
    memlayout::{DeviceMappingInfo, TRAMPOLINE, TRAPFRAME},
    vm::{AccessFlags, PageInitiator, PageTable, PageTableEntryDesc, RawPageTable},
};

extern "C" {
    // trampoline.S
    static mut trampoline: [u8; 0];
}

bitflags! {
    pub struct RiscVPteFlags: usize {
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

pub type PteFlags = RiscVPteFlags;

impl From<AccessFlags> for RiscVPteFlags {
    fn from(item: AccessFlags) -> Self {
        let mut ret = Self::empty();
        if item.intersects(AccessFlags::R) {
            ret |= Self::R;
        }
        if item.intersects(AccessFlags::W) {
            ret |= Self::W;
        }
        if item.intersects(AccessFlags::X) {
            ret |= Self::X;
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
pub struct RiscVPageTableEntry {
    inner: usize,
}

pub type PageTableEntry = RiscVPageTableEntry;

impl PageTableEntryDesc for RiscVPageTableEntry {
    type EntryFlags = RiscVPteFlags;

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
            && !self
                .flag_intersects(Self::EntryFlags::R | Self::EntryFlags::W | Self::EntryFlags::X)
    }

    fn is_data(&self) -> bool {
        self.is_valid()
            && self.flag_intersects(Self::EntryFlags::R | Self::EntryFlags::W | Self::EntryFlags::X)
    }

    /// Make the entry refer to a given page-table page.
    fn set_table(&mut self, page: *mut RawPageTable) {
        self.inner = pa2pte((page as usize).into()) | Self::EntryFlags::V.bits();
    }

    /// Make the entry refer to a given address with a given permission.
    /// The permission should include at lease one of R, W, and X not to be
    /// considered as an entry referring a page-table page.
    fn set_entry(&mut self, pa: PAddr, perm: Self::EntryFlags) {
        assert!(perm.intersects(Self::EntryFlags::R | Self::EntryFlags::W | Self::EntryFlags::X));
        self.inner = pa2pte(pa) | (perm | Self::EntryFlags::V).bits();
    }

    /// Make the entry inaccessible by user processes by clearing RiscVPteFlags::U.
    fn clear_user(&mut self) {
        self.inner &= !(Self::EntryFlags::U.bits());
    }

    /// Invalidate the entry by making every bit 0.
    fn invalidate(&mut self) {
        self.inner = 0;
    }
}

pub struct ArmV8PageInit;

pub type PageInit = ArmV8PageInit;

impl PageInitiator for ArmV8PageInit {
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
            TRAMPOLINE.into(),
            // SAFETY: we assume that reading the address of trampoline is safe.
            (unsafe { trampoline.as_mut_ptr() as usize }).into(),
            RiscVPteFlags::R | RiscVPteFlags::X,
            allocator,
        )?;

        // Map the trapframe just below TRAMPOLINE, for trampoline.S.
        page_table.insert(
            TRAPFRAME.into(),
            trap_frame,
            RiscVPteFlags::R | RiscVPteFlags::W,
            allocator,
        )?;

        Ok(())
    }

    fn kernel_page_init<A: VAddr>(
        page_table: &mut PageTable<A>,
        allocator: Pin<&SpinLock<Kmem>>,
    ) -> Result<(), ()> {
        // SiFive Test Finisher MMIO
        page_table.insert_range(
            FINISHER.into(),
            PGSIZE,
            FINISHER.into(),
            RiscVPteFlags::R | RiscVPteFlags::W,
            allocator,
        )?;

        // Uart registers
        page_table.insert_range(
            MemLayout::UART0.into(),
            PGSIZE,
            MemLayout::UART0.into(),
            RiscVPteFlags::R | RiscVPteFlags::W,
            allocator,
        )?;

        // Virtio mmio disk interface
        page_table.insert_range(
            MemLayout::VIRTIO0.into(),
            PGSIZE,
            MemLayout::VIRTIO0.into(),
            RiscVPteFlags::R | RiscVPteFlags::W,
            allocator,
        )?;

        // PLIC
        page_table.insert_range(
            PLIC.into(),
            0x400000,
            PLIC.into(),
            RiscVPteFlags::R | RiscVPteFlags::W,
            allocator,
        )?;
        // Map the trampoline for trap entry/exit to
        // the highest virtual address in the kernel.
        page_table.insert_range(
            TRAMPOLINE.into(),
            PGSIZE,
            // SAFETY: we assume that reading the address of trampoline is safe.
            unsafe { trampoline.as_mut_ptr() as usize }.into(),
            RiscVPteFlags::R | RiscVPteFlags::X,
            allocator,
        )?;

        Ok(())
    }

    unsafe fn switch_page_table_and_enable_mmu(page_table_base: usize) {
        unsafe {
            w_satp(make_satp(page_table_base));
            sfence_vma();
        }
    }
}
