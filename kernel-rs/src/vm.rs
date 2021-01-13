use crate::{
    kernel::kernel,
    kernel::Kernel,
    memlayout::{FINISHER, KERNBASE, PHYSTOP, PLIC, TRAMPOLINE, UART0, VIRTIO0},
    page::Page,
    proc::{myproc, proc_mapstacks},
    riscv::{
        make_satp, pa2pte, pgrounddown, pgroundup, pte2pa, pte_flags, px, sfence_vma, w_satp, PteT,
        MAXVA, PGSIZE, PTE_R, PTE_U, PTE_V, PTE_W, PTE_X,
    },
    some_or,
};
use core::{marker::PhantomData, mem, ops::Add, ptr};

extern "C" {
    // kernel.ld sets this to end of kernel code.
    static mut etext: [u8; 0];

    // trampoline.S
    static mut trampoline: [u8; 0];
}

#[derive(Clone, Copy)]
pub struct PAddr(usize);

#[derive(Clone, Copy)]
pub struct KVAddr(usize);

#[derive(Clone, Copy)]
pub struct UVAddr(usize);

impl PAddr {
    pub const fn new(value: usize) -> Self {
        PAddr(value)
    }

    pub const fn into_usize(self) -> usize {
        self.0
    }
}

impl Add<usize> for KVAddr {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl Add<usize> for UVAddr {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 + rhs)
    }
}

pub trait VAddr: Copy + Add<usize, Output = Self> {
    fn new(value: usize) -> Self;

    fn into_usize(&self) -> usize;

    fn is_null(&self) -> bool;

    fn is_page_aligned(&self) -> bool;

    /// Copy from either a user address, or kernel address.
    /// Returns Ok(()) on success, Err(()) on error.
    unsafe fn copyin(dst: &mut [u8], src: Self) -> Result<(), ()>;

    /// Copy to either a user address, or kernel address.
    /// Returns Ok(()) on success, Err(()) on error.
    unsafe fn copyout(dst: Self, src: &[u8]) -> Result<(), ()>;
}

impl VAddr for KVAddr {
    fn new(value: usize) -> Self {
        KVAddr(value)
    }

    fn into_usize(&self) -> usize {
        self.0
    }

    fn is_null(&self) -> bool {
        self.0 == 0
    }

    fn is_page_aligned(&self) -> bool {
        self.0 % PGSIZE == 0
    }

    unsafe fn copyin(dst: &mut [u8], src: Self) -> Result<(), ()> {
        ptr::copy(src.into_usize() as *const u8, dst.as_mut_ptr(), dst.len());
        Ok(())
    }

    unsafe fn copyout(dst: Self, src: &[u8]) -> Result<(), ()> {
        ptr::copy(src.as_ptr(), dst.into_usize() as *mut u8, src.len());
        Ok(())
    }
}

impl VAddr for UVAddr {
    fn new(value: usize) -> Self {
        UVAddr(value)
    }

    fn into_usize(&self) -> usize {
        self.0
    }

    fn is_null(&self) -> bool {
        self.0 == 0
    }

    fn is_page_aligned(&self) -> bool {
        self.0 % PGSIZE == 0
    }

    unsafe fn copyin(dst: &mut [u8], src: Self) -> Result<(), ()> {
        let p = myproc();
        (*(*p).data.get())
            .pagetable
            .copy_in(dst, src)
            .map_or(Err(()), |_v| Ok(()))
    }

    unsafe fn copyout(dst: Self, src: &[u8]) -> Result<(), ()> {
        let p = myproc();
        (*(*p).data.get())
            .pagetable
            .copy_out(dst, src)
            .map_or(Err(()), |_v| Ok(()))
    }
}

#[derive(Default)]
struct PageTableEntry {
    inner: PteT,
}

impl PageTableEntry {
    fn get_flags(&self) -> usize {
        pte_flags(self.inner)
    }

    fn check_flag(&self, flag: usize) -> bool {
        self.inner & flag != 0
    }

    fn set_flag(&mut self, flag: usize) {
        self.inner |= flag;
    }

    fn clear_flag(&mut self, flag: usize) {
        self.inner &= !flag;
    }

    fn set_inner(&mut self, inner: PteT) {
        self.inner = inner;
    }

    fn get_pa(&self) -> PAddr {
        pte2pa(self.inner)
    }

    fn is_valid(&self) -> bool {
        self.check_flag(PTE_V)
    }

    fn is_table(&self) -> bool {
        self.is_valid() && !self.check_flag((PTE_R | PTE_W | PTE_X) as usize)
    }

    fn is_data(&self) -> bool {
        self.is_valid() && self.check_flag((PTE_R | PTE_W | PTE_X) as usize)
    }

    /// # Safety
    ///
    /// If `self.is_table()` is true, then it must refer to a valid page-table page.
    ///
    /// Return `Some(..)` if it refers to a page-table page.
    /// Return `None` if it refers to a data page.
    /// Return `None` if it is invalid.
    unsafe fn as_table_mut(&mut self) -> Option<&mut RawPageTable> {
        if self.is_table() {
            (pte2pa(self.inner).into_usize() as *mut RawPageTable).as_mut()
        } else {
            None
        }
    }
}

const PTE_PER_PT: usize = PGSIZE / mem::size_of::<PageTableEntry>();

struct RawPageTable {
    // Internal safety invariant:
    // If an entry's V flag is set but its RWX flags are not set,
    // then it must refer to a valid page-table page.
    // It should be safely converted to a Page without breaking the invariants
    // of Page.
    // It should not be accessed outside RawPageTable to guarantee the invariant.
    inner: [PageTableEntry; PTE_PER_PT],
}

impl RawPageTable {
    /// Make a new emtpy raw page table by allocating a new page.
    /// Return `Ok(..)` if the allocation has succeeded.
    /// Return `None` if the allocation has failed.
    fn new() -> Option<*mut RawPageTable> {
        let mut page = kernel().alloc()?;
        page.write_bytes(0);
        Some(page.into_usize() as *mut RawPageTable)
    }

    /// Return `Some(..)` if the `index`th entry refers to a page-table page.
    /// Return `Some(..)` by allocating a new page if the `index`th
    /// entry is invalid but `alloc` is true. The result becomes `None` when the
    /// allocation has failed.
    /// Return `None` if the `index`th entry refers to a data page.
    /// Return `None` if the `index`th entry is invalid and `alloc` is false.
    fn get_table_mut(&mut self, index: usize, alloc: bool) -> Option<&mut RawPageTable> {
        let pte = &mut self.inner[index];
        if !pte.is_valid() {
            if !alloc {
                return None;
            }
            let page = Self::new()?;
            let k = page as usize;
            pte.set_inner(pa2pte(PAddr::new(k)) | PTE_V);
        }
        // It is safe because of the RawPageTable's invariant.
        unsafe { pte.as_table_mut() }
    }

    /// Return a `PageTableEntry` if the `index`th entry refers to a data page.
    /// Return a `PageTableEntry` if the `index`th entry is invalid.
    /// Panic if the `index`th entry refers to a page-table page.
    fn get_entry_mut(&mut self, index: usize) -> &mut PageTableEntry {
        let pte = &mut self.inner[index];
        assert!(!pte.is_table());
        pte
    }

    /// Recursively free page-table pages.
    /// All leaf mappings must already have been removed.
    /// This method frees the page table itself, so this page table must
    /// not be used after an invocation of this method.
    #[deny(unsafe_op_in_unsafe_fn)]
    unsafe fn free_walk(&mut self) {
        // There are 2^9 = 512 PTEs in a page table.
        for pte in &mut self.inner {
            // It is safe because of the RawPageTable's invariant.
            if let Some(ptable) = unsafe { pte.as_table_mut() } {
                // It is safe because ptable will not be used anymore.
                unsafe { ptable.free_walk() };
                pte.set_inner(0);
            }
        }
        // It is safe to convert inner to a Page because of the invariant.
        let page = unsafe { Page::from_usize(self.inner.as_mut_ptr() as _) };
        kernel().free(page);
    }
}

pub struct PageTable<A> {
    // Internal safety invariant:
    // ptr uniquely refers to a valid 3-level RawPageTable.
    ptr: *mut RawPageTable,
    _marker: PhantomData<A>,
}

impl<A: VAddr> PageTable<A> {
    // TODO(rv6): it remains for initialization of ProcData.
    // When ProcData changes to have Option<PageTable<_>> instead of
    // PageTable<_>, this method can be removed.
    pub const unsafe fn zero() -> Self {
        Self {
            ptr: ptr::null_mut(),
            _marker: PhantomData,
        }
    }

    /// Make a new empty page table by allocating a new page.
    /// Return `Ok(..)` if the allocation has succeeded.
    /// Return `None` if the allocation has failed.
    pub fn new() -> Option<Self> {
        Some(Self {
            ptr: RawPageTable::new()?,
            _marker: PhantomData,
        })
    }

    pub fn as_usize(&self) -> usize {
        self.ptr as usize
    }

    fn as_raw_mut(&mut self) -> &mut RawPageTable {
        // It is safe because self.ptr uniquely refers to a valid RawPageTable
        // according to the invariant.
        unsafe { self.ptr.as_mut() }.expect("ptr must not be null")
    }

    /// Return the reference of the PTE in this page table
    /// that corresponds to virtual address `va`. If `alloc` is true,
    /// create any required page-table pages.
    ///
    /// The risc-v Sv39 scheme has three levels of page-table
    /// pages. A page-table page contains 512 64-bit PTEs.
    /// A 64-bit virtual address is split into five fields:
    ///   39..63 -- must be zero.
    ///   30..38 -- 9 bits of level-2 index.
    ///   21..29 -- 9 bits of level-1 index.
    ///   12..20 -- 9 bits of level-0 index.
    ///    0..11 -- 12 bits of byte offset within the page.
    fn walk(&mut self, va: A, alloc: bool) -> Option<&mut PageTableEntry> {
        assert!(va.into_usize() < MAXVA, "walk");
        let mut page_table = self.as_raw_mut();
        for level in (1..3).rev() {
            page_table = page_table.get_table_mut(px(level, va), alloc)?;
        }
        Some(page_table.get_entry_mut(px(0, va)))
    }

    /// Create PTEs for virtual addresses starting at va that refer to
    /// physical addresses starting at pa. va and size might not
    /// be page-aligned. Returns Ok(()) on success, Err(()) if walk() couldn't
    /// allocate a needed page-table page.
    pub fn map_pages(&mut self, va: A, size: usize, mut pa: usize, perm: i32) -> Result<(), ()> {
        let mut a = pgrounddown(va.into_usize());
        let last = pgrounddown(va.into_usize() + size - 1usize);
        loop {
            let pte = self.walk(VAddr::new(a), true).ok_or(())?;
            assert!(!pte.is_valid(), "remap");

            pte.set_inner(pa2pte(PAddr::new(pa)) | perm as usize | PTE_V);
            if a == last {
                break;
            }
            a += PGSIZE;
            pa += PGSIZE;
        }
        Ok(())
    }
}

impl PageTable<UVAddr> {
    /// Look up a virtual address, return Some(physical address),
    /// or None if not mapped.
    pub fn uvm_walk_addr(&mut self, va: UVAddr) -> Option<PAddr> {
        if va.into_usize() >= MAXVA {
            return None;
        }
        let pte = self.walk(va, false)?;
        if !pte.is_valid() {
            return None;
        }
        if !pte.check_flag(PTE_U as usize) {
            return None;
        }
        Some(pte.get_pa())
    }

    /// Load the user initcode into address 0 of pagetable,
    /// for the very first process.
    /// sz must be less than a page.
    pub unsafe fn uvm_init(&mut self, src: &[u8]) {
        assert!(src.len() < PGSIZE, "inituvm: more than a page");

        let mem = kernel().alloc().unwrap().into_usize() as *mut u8;
        ptr::write_bytes(mem, 0, PGSIZE);
        self.map_pages(
            VAddr::new(0),
            PGSIZE,
            mem as usize,
            PTE_W | PTE_R | PTE_X | PTE_U,
        )
        .expect("inituvm: mappage");
        ptr::copy(src.as_ptr(), mem, src.len());
    }

    /// Allocate PTEs and physical memory to grow process from oldsz to
    /// newsz, which need not be page aligned.  Returns Ok(new size) or Err(()) on error.
    pub fn uvm_alloc(&mut self, mut oldsz: usize, newsz: usize) -> Result<usize, ()> {
        if newsz < oldsz {
            return Ok(oldsz);
        }
        oldsz = pgroundup(oldsz);
        let mut a = oldsz;
        while a < newsz {
            let mut mem = some_or!(kernel().alloc(), {
                self.uvm_dealloc(a, oldsz);
                return Err(());
            });
            mem.write_bytes(0);
            let pa = mem.into_usize();
            if self
                .map_pages(VAddr::new(a), PGSIZE, pa, PTE_W | PTE_X | PTE_R | PTE_U)
                .is_err()
            {
                // It is safe because pa is an address of mem, which is a page
                // obtained by alloc().
                kernel().free(unsafe { Page::from_usize(pa) });
                self.uvm_dealloc(a, oldsz);
                return Err(());
            }
            a += PGSIZE;
        }
        Ok(newsz)
    }

    /// Given a parent process's page table, copy
    /// its memory into a child's page table.
    /// Copies both the page table and the
    /// physical memory.
    /// Returns Ok(()) on success, Err(()) on failure.
    /// Frees any allocated pages on failure.
    pub unsafe fn uvm_copy(
        &mut self,
        mut new: &mut PageTable<UVAddr>,
        sz: usize,
    ) -> Result<(), ()> {
        for i in num_iter::range_step(0, sz, PGSIZE) {
            let pte = self
                .walk(UVAddr::new(i), false)
                .expect("uvmcopy: pte should exist");
            assert!(pte.is_valid(), "uvmcopy: page not present");

            let mut new_ptable = scopeguard::guard(new, |ptable| {
                ptable.uvm_unmap(UVAddr::new(0), i.wrapping_div(PGSIZE), true);
            });
            let pa = pte.get_pa();
            let flags = pte.get_flags() as u32;
            let mem = kernel().alloc().ok_or(())?.into_usize();
            ptr::copy(
                pa.into_usize() as *mut u8 as *const u8,
                mem as *mut u8,
                PGSIZE,
            );
            if (*new_ptable)
                .map_pages(VAddr::new(i), PGSIZE, mem as usize, flags as i32)
                .is_err()
            {
                kernel().free(Page::from_usize(mem as _));
                return Err(());
            }
            new = scopeguard::ScopeGuard::into_inner(new_ptable);
        }
        Ok(())
    }

    /// Remove npages of mappings starting from va. va must be
    /// page-aligned. The mappings must exist.
    /// Optionally free the physical memory.
    pub fn uvm_unmap(&mut self, va: UVAddr, npages: usize, do_free: bool) {
        if va.into_usize().wrapping_rem(PGSIZE) != 0 {
            panic!("uvmunmap: not aligned");
        }
        let start = va.into_usize();
        let end = start.wrapping_add(npages.wrapping_mul(PGSIZE));
        for a in num_iter::range_step(start, end, PGSIZE) {
            let pt = &mut *self;
            let pte = pt.walk(UVAddr::new(a), false).expect("uvmunmap: walk");
            assert!(pte.is_data(), "uvmunmap: not a valid leaf");

            if do_free {
                let pa = pte.get_pa().into_usize();
                // TODO(rv6)
                // We do not know anything about pa, so, for now, we cannot
                // guarantee that it is safe.
                kernel().free(unsafe { Page::from_usize(pa) });
            }
            pte.set_inner(0);
        }
    }

    /// Deallocate user pages to bring the process size from oldsz to
    /// newsz.  oldsz and newsz need not be page-aligned, nor does newsz
    /// need to be less than oldsz.  oldsz can be larger than the actual
    /// process size.  Returns the new process size.
    pub fn uvm_dealloc(&mut self, oldsz: usize, newsz: usize) -> usize {
        if newsz >= oldsz {
            return oldsz;
        }

        if pgroundup(newsz) < pgroundup(oldsz) {
            let npages = (pgroundup(oldsz).wrapping_sub(pgroundup(newsz))).wrapping_div(PGSIZE);
            self.uvm_unmap(UVAddr::new(pgroundup(newsz)), npages, true);
        }
        newsz
    }

    /// Free user memory pages,
    /// then free page-table pages.
    pub fn uvm_free(mut self, sz: usize) {
        if sz > 0 {
            self.uvm_unmap(UVAddr::new(0), pgroundup(sz).wrapping_div(PGSIZE), true);
        }
        // It is safe because this method consumes self, so the internal
        // raw page table will not be use anymore.
        unsafe { self.as_raw_mut().free_walk() };
    }

    /// Mark a PTE invalid for user access.
    /// Used by exec for the user stack guard page.
    pub fn uvm_guard(&mut self, va: UVAddr) {
        self.walk(va, false)
            .expect("uvmguard")
            .clear_flag(PTE_U as usize);
    }

    /// Copy from kernel to user.
    /// Copy len bytes from src to virtual address dstva in a given page table.
    /// Return Ok(()) on success, Err(()) on error.
    pub unsafe fn copy_out(&mut self, dstva: UVAddr, src: &[u8]) -> Result<(), ()> {
        let mut dst = dstva.into_usize();
        let mut len = src.len();
        let mut offset = 0;
        while len > 0 {
            let va0 = pgrounddown(dst);
            let pa0 = self.uvm_walk_addr(VAddr::new(va0)).ok_or(())?.into_usize();
            let mut n = PGSIZE - (dst - va0);
            if n > len {
                n = len
            }
            ptr::copy(
                src[offset..(offset + n)].as_ptr(),
                (pa0 + (dst - va0)) as *mut u8,
                n,
            );
            len -= n;
            offset += n;
            dst = va0 + PGSIZE;
        }
        Ok(())
    }

    /// Copy from user to kernel.
    /// Copy len bytes to dst from virtual address srcva in a given page table.
    /// Return Ok(()) on success, Err(()) on error.
    pub unsafe fn copy_in(&mut self, dst: &mut [u8], srcva: UVAddr) -> Result<(), ()> {
        let mut src = srcva.into_usize();
        let mut len = dst.len();
        let mut offset = 0;
        while len > 0 {
            let va0 = pgrounddown(src);
            let pa0 = self.uvm_walk_addr(VAddr::new(va0)).ok_or(())?.into_usize();
            let mut n = PGSIZE - (src - va0);
            if n > len {
                n = len
            }
            ptr::copy(
                (pa0 + (src - va0)) as *mut u8,
                dst[offset..(offset + n)].as_mut_ptr(),
                n,
            );
            len -= n;
            offset += n;
            src = va0 + PGSIZE
        }
        Ok(())
    }

    /// Copy a null-terminated string from user to kernel.
    /// Copy bytes to dst from virtual address srcva in a given page table,
    /// until a '\0', or max.
    /// Return OK(()) on success, Err(()) on error.
    pub unsafe fn copy_in_str(&mut self, dst: &mut [u8], srcva: UVAddr) -> Result<(), ()> {
        let mut got_null: i32 = 0;
        let mut src = srcva.into_usize();
        let mut offset = 0;
        let mut max = dst.len();
        while got_null == 0 && max > 0 {
            let va0 = pgrounddown(src);
            let pa0 = self.uvm_walk_addr(VAddr::new(va0)).ok_or(())?.into_usize();
            let mut n = PGSIZE - (src - va0);
            if n > max {
                n = max
            }
            let mut p = (pa0 + (src - va0)) as *mut u8;
            while n > 0 {
                if *p as i32 == '\u{0}' as i32 {
                    dst[offset] = '\u{0}' as i32 as u8;
                    got_null = 1;
                    break;
                } else {
                    dst[offset] = *p;
                    n -= 1;
                    max -= 1;
                    p = p.offset(1);
                    offset += 1;
                }
            }
            src = va0 + PGSIZE
        }
        if got_null != 0 {
            Ok(())
        } else {
            Err(())
        }
    }
}

impl PageTable<KVAddr> {
    /// Make a direct-map page table for the kernel.
    pub fn kvm_new() -> Option<Self> {
        let mut page_table = Self::new()?;
        page_table.kvm_make();
        Some(page_table)
    }

    /// Add direct-mappings for the kernel to this page table.
    pub fn kvm_make(&mut self) {
        // SiFive Test Finisher MMIO
        self.kvm_map(
            KVAddr::new(FINISHER),
            PAddr::new(FINISHER),
            PGSIZE,
            PTE_R | PTE_W,
        );

        // Uart registers
        self.kvm_map(KVAddr::new(UART0), PAddr::new(UART0), PGSIZE, PTE_R | PTE_W);

        // Virtio mmio disk interface
        self.kvm_map(
            KVAddr::new(VIRTIO0),
            PAddr::new(VIRTIO0),
            PGSIZE,
            PTE_R | PTE_W,
        );

        // PLIC
        self.kvm_map(KVAddr::new(PLIC), PAddr::new(PLIC), 0x400000, PTE_R | PTE_W);

        // Map kernel text executable and read-only.
        let et = unsafe { etext.as_mut_ptr() as usize };
        self.kvm_map(
            KVAddr::new(KERNBASE),
            PAddr::new(KERNBASE),
            et - KERNBASE,
            PTE_R | PTE_X,
        );

        // Map kernel data and the physical RAM we'll make use of.
        self.kvm_map(KVAddr::new(et), PAddr::new(et), PHYSTOP - et, PTE_R | PTE_W);

        // Map the trampoline for trap entry/exit to
        // the highest virtual address in the kernel.
        self.kvm_map(
            KVAddr::new(TRAMPOLINE),
            PAddr::new(unsafe { trampoline.as_mut_ptr() as usize }),
            PGSIZE,
            PTE_R | PTE_X,
        );

        // map kernel stacks
        proc_mapstacks(self);
    }

    /// Switch h/w page table register to the kernel's page table,
    /// and enable paging.
    pub unsafe fn kvm_init_hart(&self) {
        w_satp(make_satp(self.ptr as usize));
        sfence_vma();
    }

    /// Add a mapping to the kernel page table.
    /// Only used when booting.
    /// Does not flush TLB or enable paging.
    pub fn kvm_map(&mut self, va: KVAddr, pa: PAddr, sz: usize, perm: i32) {
        self.map_pages(va, sz, pa.into_usize(), perm)
            .expect("kvm_map");
    }
}

impl Kernel {
    pub unsafe fn kvm_init_hart(&self) {
        self.page_table
            .as_ref()
            .expect("kernel page table must not be None")
            .kvm_init_hart();
    }
}
