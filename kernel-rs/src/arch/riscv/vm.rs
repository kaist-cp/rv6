use core::pin::Pin;

use bitflags::bitflags;

use crate::{
    addr::{pa2pte, pte2pa, PAddr, VAddr, PGSIZE},
    arch::asm::{make_satp, sfence_vma, w_satp},
<<<<<<< HEAD
    arch::memlayout::{
        kstack, FINISHER, KERNBASE, PHYSTOP, PLIC, TRAMPOLINE, TRAPFRAME, UART0, VIRTIO0,
    },
    arch::riscv::{make_satp, sfence_vma, w_satp},
    fs::{DefaultFs, InodeGuard},
    arch::asm::{make_satp, sfence_vma, w_satp},
    fs::{FileSystem, InodeGuard, Ufs},
=======
    arch::memlayout::{FINISHER, PLIC, TRAMPOLINE, TRAPFRAME, UART0, VIRTIO0},
>>>>>>> refactoring: divide vm.rs
    kalloc::Kmem,
    lock::SpinLock,
    vm::{AccessFlags, PageInit, PageTable, PageTableEntry, PteFlags, RawPageTable},
};

extern "C" {
    // trampoline.S
    static mut trampoline: [u8; 0];
}

bitflags! {
    pub struct PteFlagsImpl: usize {
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

impl PteFlags for PteFlagsImpl {
    fn from_access_flags(f: AccessFlags) -> Self {
        let mut ret = Self::empty();
        if f.intersects(AccessFlags::R) {
            ret |= Self::R;
        }
        if f.intersects(AccessFlags::W) {
            ret |= Self::W;
        }
        if f.intersects(AccessFlags::X) {
            ret |= Self::X;
        }
        if f.intersects(AccessFlags::U) {
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
pub struct PageTableEntryImpl {
    inner: usize,
}

impl PageTableEntry for PageTableEntryImpl {
    type EntryFlags = PteFlagsImpl;

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

    /// Make the entry inaccessible by user processes by clearing PteFlagsImpl::U.
    fn clear_user(&mut self) {
        self.inner &= !(Self::EntryFlags::U.bits());
    }

    /// Invalidate the entry by making every bit 0.
    fn invalidate(&mut self) {
        self.inner = 0;
    }
}

pub struct PageInitImpl {}

impl PageInit for PageInitImpl {
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
            PteFlagsImpl::R | PteFlagsImpl::X,
            allocator,
        )?;

        // Map the trapframe just below TRAMPOLINE, for trampoline.S.
        page_table.insert(
            TRAPFRAME.into(),
            trap_frame,
            PteFlagsImpl::R | PteFlagsImpl::W,
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
            PteFlagsImpl::R | PteFlagsImpl::W,
            allocator,
        )?;

        // Uart registers
        page_table.insert_range(
            UART0.into(),
            PGSIZE,
            UART0.into(),
            PteFlagsImpl::R | PteFlagsImpl::W,
            allocator,
        )?;

        // Virtio mmio disk interface
        page_table.insert_range(
            VIRTIO0.into(),
            PGSIZE,
            VIRTIO0.into(),
            PteFlagsImpl::R | PteFlagsImpl::W,
            allocator,
        )?;

        // PLIC
        page_table.insert_range(
            PLIC.into(),
            0x400000,
            PLIC.into(),
            PteFlagsImpl::R | PteFlagsImpl::W,
            allocator,
        )?;
        // Map the trampoline for trap entry/exit to
        // the highest virtual address in the kernel.
        page_table.insert_range(
            TRAMPOLINE.into(),
            PGSIZE,
            // SAFETY: we assume that reading the address of trampoline is safe.
            unsafe { trampoline.as_mut_ptr() as usize }.into(),
            PteFlagsImpl::R | PteFlagsImpl::X,
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
