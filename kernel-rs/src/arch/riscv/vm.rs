use core::{cmp, marker::PhantomData, mem, pin::Pin, slice};

use bitflags::bitflags;
use zerocopy::{AsBytes, FromBytes};

use crate::{
    addr::{
        pa2pte, pte2pa, KVAddr, PAddr, UVAddr, VAddr, MAXVA, PGSIZE,
    },
    arch::asm::{make_satp, sfence_vma, w_satp},
    arch::memlayout::{
        kstack, FINISHER, KERNBASE, PHYSTOP, PLIC, TRAMPOLINE, TRAPFRAME, UART0, VIRTIO0,
    },
    arch::riscv::{make_satp, sfence_vma, w_satp},
    fs::{DefaultFs, InodeGuard},
    arch::asm::{make_satp, sfence_vma, w_satp},
    fs::{FileSystem, InodeGuard, Ufs},
    kalloc::Kmem,
    lock::SpinLock,
    page::Page,
    param::NPROC,
    proc::KernelCtx,
    vm::{PageTableEntry, PageInit, PteFlags, AccessFlags, RawPageTable, PageTable},
};

extern "C" {
    // kernel.ld sets this to end of kernel code.
    static mut etext: [u8; 0];

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
        self.is_valid() && !self.flag_intersects(Self::EntryFlags::R | Self::EntryFlags::W | Self::EntryFlags::X)
    }

    fn is_data(&self) -> bool {
        self.is_valid() && self.flag_intersects(Self::EntryFlags::R | Self::EntryFlags::W | Self::EntryFlags::X)
    }

    /// Make the entry refer to a given page-table page.
    fn set_table(&mut self, page: *mut RawPageTable) {
        self.inner = pa2pte((page as usize).into()) | Self::EntryFlags::V.bits();
    }

    /// Make the entry refer to a given address with a given permission.
    /// The permission should include at lease one of R, W, and X not to be
    /// considered as an entry referring a page-table page.
    fn set_entry(&mut self, pa: PAddr, perm: Self::Self::EntryFlags) {
        assert!(perm.intersects(Self::EntryFlags::R | Self::EntryFlags::W | Self::EntryFlags::X));
        self.inner = pa2pte(pa) | (perm | Self::EntryFlags::V).bits();
    }

    /// Make the entry inaccessible by user processes by clearing PteFlags::U.
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
    fn user_page_init(page_table: &mut PageTable, trap_frame: PAddr, allocator: Pin<&SpinLock<Kmem>>) {
        // Map the trampoline code (for system call return)
        // at the highest user virtual address.
        // Only the supervisor uses it, on the way
        // to/from user space, so not PTE_U.
        page_table
            .insert(
                TRAMPOLINE.into(),
                // SAFETY: we assume that reading the address of trampoline is safe.
                (unsafe { trampoline.as_mut_ptr() as usize }).into(),
                PteFlags::R | PteFlags::X,
                allocator,
            )
            .ok()?;

        // Map the trapframe just below TRAMPOLINE, for trampoline.S.
        page_table
            .insert(
                TRAPFRAME.into(),
                trap_frame,
                PteFlags::R | PteFlags::W,
                allocator,
            )
            .ok()?;

        let mut memory = Self {
            page_table: scopeguard::ScopeGuard::into_inner(page_table),
            size: 0,
        };

        if let Some(src) = src_opt {
            assert!(src.len() < PGSIZE, "new: more than a page");
            let mut page = allocator.alloc()?;
            page.write_bytes(0);
            (&mut page[..src.len()]).copy_from_slice(src);
            memory
                .push_page(
                    page,
                    PteFlags::R | PteFlags::W | PteFlags::X | PteFlags::U,
                    allocator,
                )
                .map_err(|page| allocator.free(page))
                .ok()?;
        }

        Some(memory)
    }

    /// Makes a new memory by copying a given memory. Copies both the page
    /// table and the physical memory. Returns Some(memory) on success, None on
    /// failure. Frees any allocated pages on failure.
    pub fn clone(&mut self, trap_frame: PAddr, allocator: Pin<&SpinLock<Kmem>>) -> Option<Self> {
        let new = Self::new(trap_frame, None, allocator)?;
        let mut new = scopeguard::guard(new, |mut new| {
            let _ = new.dealloc(0, allocator);
        });
        for i in num_iter::range_step(0, self.size, PGSIZE) {
            let pte = self
                .page_table
                .get_mut(i.into(), None)
                .expect("clone_into: pte not found");
            assert!(pte.is_valid(), "clone_into: invalid page");

            let pa = pte.get_pa();
            let flags = pte.get_flags();
            let mut page = allocator.alloc()?;
            // SAFETY: pa is an address in page_table,
            // and thus it is the address of a page by the invariant.
            let src = unsafe { slice::from_raw_parts(pa.into_usize() as *const u8, PGSIZE) };
            page.copy_from_slice(src);
            new.push_page(page, flags, allocator)
                .map_err(|page| allocator.free(page))
                .ok()?;
        }
        let mut new = scopeguard::ScopeGuard::into_inner(new);
        new.size = self.size;
        Some(new)
    }

    /// Get the size of this memory.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Load data from a file into memory at virtual address va. va must be
    /// page-aligned, and the pages from va to va + sz must already be mapped.
    ///
    /// Returns Ok(()) on success, Err(()) on failure.
    pub fn load_file(
        &mut self,
        va: UVAddr,
        ip: &mut InodeGuard<'_, DefaultFs>,
        offset: u32,
        sz: u32,
        ctx: &KernelCtx<'_, '_>,
    ) -> Result<(), ()> {
        assert!(va.is_page_aligned(), "load_file: va must be page aligned");
        for i in num_iter::range_step(0, sz, PGSIZE as _) {
            let dst = self
                .get_slice(va + i as usize)
                .expect("load_file: address should exist");
            let n = cmp::min((sz - i) as usize, PGSIZE);
            let bytes_read = ip.read_bytes_kernel(&mut dst[..n], offset + i, ctx);
            if bytes_read != n {
                return Err(());
            }
        }
        Ok(())
    }

    /// Allocate PTEs and physical memory to grow process to newsz, which need
    /// not be page aligned. Returns Ok(new size) or Err(()) on error.
    pub fn alloc(&mut self, newsz: usize, allocator: Pin<&SpinLock<Kmem>>) -> Result<usize, ()> {
        if newsz <= self.size {
            return Ok(self.size);
        }

        let oldsz = self.size;
        let mut this = scopeguard::guard(self, |this| {
            let _ = this.dealloc(oldsz, allocator);
        });
        // this.size = pgroundup(this.size);
        // while this.size < newsz {
        while pgroundup(this.size) < pgroundup(newsz) {
            let mut page = allocator.alloc().ok_or(())?;
            page.write_bytes(0);
            this.push_page(
                page,
                PteFlags::R | PteFlags::W | PteFlags::X | PteFlags::U,
                allocator,
            )
            .map_err(|page| allocator.free(page))?;
        }
        let this = scopeguard::ScopeGuard::into_inner(this);
        this.size = newsz;
        Ok(this.size)
    }

    /// Deallocate user pages to bring the process size to newsz, which need
    /// not be page-aligned. Returns the new process size.
    pub fn dealloc(&mut self, newsz: usize, allocator: Pin<&SpinLock<Kmem>>) -> usize {
        if self.size <= newsz {
            return self.size;
        }

        while pgroundup(newsz) < pgroundup(self.size) {
            if let Some(page) = self.pop_page() {
                allocator.free(page);
            }
        }
        self.size = newsz;
        newsz
    }

    /// Grow or shrink process size by n bytes.
    /// Return Ok(old size) on success, Err(()) on failure.
    pub fn resize(&mut self, n: i32, allocator: Pin<&SpinLock<Kmem>>) -> Result<usize, ()> {
        let size = self.size;
        match n.cmp(&0) {
            cmp::Ordering::Equal => (),
            cmp::Ordering::Greater => {
                let _ = self.alloc(size + n as usize, allocator)?;
            }
            cmp::Ordering::Less => {
                let _ = self.dealloc(size - (-n as usize), allocator);
            }
        };
        Ok(size)
    }

    /// Mark a PTE invalid for user access.
    /// Used by exec for the user stack guard page.
    pub fn clear(&mut self, va: UVAddr) {
        self.page_table
            .get_mut(va, None)
            .expect("clear")
            .clear_user();
    }

    /// Copy from kernel to user.
    /// Copy len bytes from src to virtual address dstva in a given page table.
    /// Return Ok(()) on success, Err(()) on error.
    pub fn copy_out_bytes(&mut self, dstva: UVAddr, src: &[u8]) -> Result<(), ()> {
        let mut dst = dstva.into_usize();
        let mut len = src.len();
        let mut offset = 0;
        while len > 0 {
            let va = pgrounddown(dst);
            let poffset = dst - va;
            let page = self.get_slice(va.into()).ok_or(())?;
            let n = cmp::min(PGSIZE - poffset, len);
            page[poffset..poffset + n].copy_from_slice(&src[offset..offset + n]);
            len -= n;
            offset += n;
            dst += n;
        }
        Ok(())
    }

    /// Copy from kernel to user.
    /// Copy from src to virtual address dstva in a given page table.
    /// Return Ok(()) on success, Err(()) on error.
    pub fn copy_out<T: AsBytes>(&mut self, dstva: UVAddr, src: &T) -> Result<(), ()> {
        self.copy_out_bytes(dstva, src.as_bytes())
    }

    /// Copy from user to kernel.
    /// Copy len bytes to dst from virtual address srcva in a given page table.
    /// Return Ok(()) on success, Err(()) on error.
    pub fn copy_in_bytes(&mut self, dst: &mut [u8], srcva: UVAddr) -> Result<(), ()> {
        let mut src = srcva.into_usize();
        let mut len = dst.len();
        let mut offset = 0;
        while len > 0 {
            let va = pgrounddown(src);
            let poffset = src - va;
            let page = self.get_slice(va.into()).ok_or(())?;
            let n = cmp::min(PGSIZE - poffset, len);
            dst[offset..offset + n].copy_from_slice(&page[poffset..poffset + n]);
            len -= n;
            offset += n;
            src += n;
        }
        Ok(())
    }

    /// Copy from user to kernel.
    /// Copy to dst from virtual address srcva in a given page table.
    /// Return Ok(()) on success, Err(()) on error.
    pub unsafe fn copy_in<T: AsBytes + FromBytes>(
        &mut self,
        dst: &mut T,
        srcva: UVAddr,
    ) -> Result<(), ()> {
        self.copy_in_bytes(dst.as_bytes_mut(), srcva)
    }

    /// Copy a null-terminated string from user to kernel.
    /// Copy bytes to dst from virtual address srcva in a given page table,
    /// until a '\0', or max.
    /// Return OK(()) on success, Err(()) on error.
    pub fn copy_in_str(&mut self, dst: &mut [u8], srcva: UVAddr) -> Result<(), ()> {
        let mut src = srcva.into_usize();
        let mut offset = 0;
        let mut max = dst.len();
        while max > 0 {
            let va = pgrounddown(src);
            let poffset = src - va;
            let page = self.get_slice(va.into()).ok_or(())?;
            let n = cmp::min(PGSIZE - poffset, max);

            let from = &page[poffset..poffset + n];
            match from.iter().position(|c| *c == 0) {
                Some(i) => {
                    dst[offset..offset + i + 1].copy_from_slice(&from[..i + 1]);
                    return Ok(());
                }
                None => {
                    dst[offset..offset + n].copy_from_slice(from);
                    max -= n;
                    offset += n;
                    src += n;
                }
            }
        }
        Err(())
    }

    /// Return the address of the page table for this memory in the riscv's sv39
    /// page table scheme.
    pub fn satp(&self) -> usize {
        make_satp(self.page_table.as_usize())
    }

    /// Return a page at va as a slice. Some(page) on success, None on failure.
    fn get_slice(&mut self, va: UVAddr) -> Option<&mut [u8]> {
        if va.into_usize() >= TRAPFRAME {
            return None;
        }
        let pte = self.page_table.get_mut(va, None)?;
        if !pte.is_user() {
            return None;
        }
        // SAFETY: va < TRAPFRAME, so pte.get_pa() is the address of a page.
        Some(unsafe { slice::from_raw_parts_mut(pte.get_pa().into_usize() as _, PGSIZE) })
    }

    fn kernel_page_init(page_table: &mut impl PageTable, allocator: Pin<&SpinLock<Kmem>>) {
// SiFive Test Finisher MMIO
page_table
.insert_range(
    FINISHER.into(),
    PGSIZE,
    FINISHER.into(),
    PteFlags::R | PteFlags::W,
    allocator,
)
.ok()?;

// Uart registers
page_table
.insert_range(
    UART0.into(),
    PGSIZE,
    UART0.into(),
    PteFlags::R | PteFlags::W,
    allocator,
)
.ok()?;

// Virtio mmio disk interface
page_table
.insert_range(
    VIRTIO0.into(),
    PGSIZE,
    VIRTIO0.into(),
    PteFlags::R | PteFlags::W,
    allocator,
)
.ok()?;

// PLIC
page_table
.insert_range(
    PLIC.into(),
    0x400000,
    PLIC.into(),
    PteFlags::R | PteFlags::W,
    allocator,
)
.ok()?;
        // Map the trampoline for trap entry/exit to
        // the highest virtual address in the kernel.
        page_table
            .insert_range(
                TRAMPOLINE.into(),
                PGSIZE,
                // SAFETY: we assume that reading the address of trampoline is safe.
                unsafe { trampoline.as_mut_ptr() as usize }.into(),
                PteFlags::R | PteFlags::X,
                allocator,
            )
            .ok()?;
    }

    unsafe fn switch_page_table_and_enable_mmu(page_table_base: usize){
        unsafe {
            w_satp(make_satp(page_table_base));
            sfence_vma();
        }
    }
}
