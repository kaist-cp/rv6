use bitflags::bitflags;
use cortex_a::registers::*;
use tock_registers::interfaces::ReadWriteable;

use crate::{
    addr::PAddr,
    arch::Armv8,
    arch::{
        addr::{pa2pte, pte2pa, PLNUM},
        asm::{isb, tlbi_vmalle1},
        interface::{IPageTableEntry, MemLayout, PageTableManager},
        memlayout::GIC,
    },
    vm::{AccessFlags, RawPageTable},
};

// A table descriptor and a level 3 page descriptor as per
// ARMv8-A Architecture Reference Manual Figure D5-15, and Figure D5-17 respectively.
bitflags! {
    pub struct PteFlags: usize {
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

// pub type PteFlags = PteFlags;

impl From<AccessFlags> for PteFlags {
    fn from(item: AccessFlags) -> Self {
        Self::ACCESS_FLAG
            | match item {
                AccessFlags::R => {
                    // Privileged Read-Only
                    Self::RO_P | Self::UXN | Self::PXN
                }
                AccessFlags::RW => {
                    // Privileged Read-Write
                    Self::RW_P | Self::UXN | Self::PXN
                }
                AccessFlags::RU => {
                    // User Read-Only
                    Self::RO_U | Self::UXN | Self::PXN
                }
                AccessFlags::RWU => {
                    // User Read-Write
                    Self::RW_U | Self::UXN | Self::PXN
                }
                AccessFlags::RX => {
                    // Privileged Read-Execute
                    Self::RO_P | Self::UXN
                }
                AccessFlags::RWX => {
                    // Privileged Read-Write-Execute
                    Self::RW_P | Self::UXN
                }
                AccessFlags::RXU => {
                    // User Read-Execute
                    Self::RO_U | Self::PXN
                }
                AccessFlags::RWXU => {
                    // User Read-Write-Execute
                    Self::RW_U | Self::PXN
                }
                _ => panic!("invalid access flag!"),
            }
    }
}

/// # Safety
///
/// If self.is_table() is true, then it must refer to a valid page-table page.
///
/// Because of #[derive(Default)], inner is initially 0, which satisfies the invariant.
#[derive(Default)]
pub struct PageTableEntry {
    inner: usize,
}

// pub type PageTableEntry = PageTableEntry;

impl IPageTableEntry for PageTableEntry {
    type EntryFlags = PteFlags;

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

impl Armv8 {
    // TODO: put ARM's counterpart of SiFive Test Finisher here
    // GIC
    const DEV_MAPPING: [(usize, usize); 1] = [(GIC, Armv8::UART0 - GIC)];
}

impl PageTableManager for Armv8 {
    type PageTableEntry = PageTableEntry;

    const PLNUM: usize = PLNUM;

    fn kernel_page_dev_mappings() -> &'static [(usize, usize)] {
        &Self::DEV_MAPPING
    }

    /// Switch h/w page table register to the kernel's page table, and enable paging.
    ///
    /// # Safety
    ///
    /// `page_table_base` must contain base address for a valid page table, containing mapping for current pc.
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
