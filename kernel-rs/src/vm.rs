use crate::{
    kernel::kernel,
    memlayout::{kstack, FINISHER, KERNBASE, PHYSTOP, PLIC, TRAMPOLINE, TRAPFRAME, UART0, VIRTIO0},
    page::Page,
    param::NPROC,
    proc::{myproc, Trapframe},
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

impl Add<usize> for PAddr {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 + rhs)
    }
}

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
    unsafe fn copy_in(dst: &mut [u8], src: Self) -> Result<(), ()>;

    /// Copy to either a user address, or kernel address.
    /// Returns Ok(()) on success, Err(()) on error.
    unsafe fn copy_out(dst: Self, src: &[u8]) -> Result<(), ()>;
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

    unsafe fn copy_in(dst: &mut [u8], src: Self) -> Result<(), ()> {
        ptr::copy(src.into_usize() as *const u8, dst.as_mut_ptr(), dst.len());
        Ok(())
    }

    unsafe fn copy_out(dst: Self, src: &[u8]) -> Result<(), ()> {
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

    unsafe fn copy_in(dst: &mut [u8], src: Self) -> Result<(), ()> {
        let p = myproc();
        (*(*p).data.get())
            .pagetable
            .copy_in(dst, src)
            .map_or(Err(()), |_v| Ok(()))
    }

    unsafe fn copy_out(dst: Self, src: &[u8]) -> Result<(), ()> {
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

    fn is_user_accessible(&self) -> bool {
        self.check_flag(PTE_V | PTE_U as usize)
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
    // TODO(rv6)
    // It is unsafe because it can be used safely only with the invariant of
    // RawPageTable. If we consider the invariant as an invariant of
    // PageTableEntry, the unsafe modifier can be removed. However, it will
    // make other methods such as set_inner unsafe.
    // https://github.com/kaist-cp/rv6/issues/339
    unsafe fn as_table_mut(&mut self) -> Option<&mut RawPageTable> {
        if self.is_table() {
            Some(&mut *(pte2pa(self.inner).into_usize() as *mut RawPageTable))
        } else {
            None
        }
    }
}

const PTE_PER_PT: usize = PGSIZE / mem::size_of::<PageTableEntry>();

/// # Safety
///
/// The invariants of this struct are as follows:
/// - If an entry's V flag is set but its RWX flags are not set,
///   then it must refer to a valid page-table page.
/// - It should be safely converted to a Page without breaking the invariants
///   of Page.
/// - It should not be accessed outside RawPageTable to guarantee the
///   invariant.
struct RawPageTable {
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

/// # Safety
///
/// The invariant of this struct is that ptr uniquely refers to a valid 3-level
/// RawPageTable.
pub struct PageTable<A: VAddr> {
    ptr: *mut RawPageTable,
    num_usr_pages: usize,
    _marker: PhantomData<A>,
}

impl<A: VAddr> PageTable<A> {
    /// # Saftey
    ///
    /// Any page table returned by this method must not be used at all.
    pub const unsafe fn zero() -> Self {
        Self {
            ptr: ptr::null_mut(),
            num_usr_pages: 0,
            _marker: PhantomData,
        }
    }

    /// Make a new empty page table by allocating a new page.
    /// Return `Ok(..)` if the allocation has succeeded.
    /// Return `None` if the allocation has failed.
    fn new_empty_table() -> Option<Self> {
        Some(Self {
            ptr: RawPageTable::new()?,
            num_usr_pages: 0,
            _marker: PhantomData,
        })
    }

    pub fn as_usize(&self) -> usize {
        self.ptr as usize
    }

    fn as_inner_mut(&mut self) -> &mut RawPageTable {
        // It is safe because self.ptr uniquely refers to a valid RawPageTable
        // according to the invariant.
        unsafe { &mut *self.ptr }
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
        let mut page_table = self.as_inner_mut();
        for level in (1..3).rev() {
            page_table = page_table.get_table_mut(px(level, va), alloc)?;
        }
        Some(page_table.get_entry_mut(px(0, va)))
    }

    /// Create PTEs for virtual addresses starting at va that refer to
    /// physical addresses starting at pa. va and size might not
    /// be page-aligned. Returns Ok(()) on success, Err(()) if walk() couldn't
    /// allocate a needed page-table page.
    fn map_pages(&mut self, va: A, size: usize, mut pa: PAddr, perm: usize) -> Result<(), ()> {
        let mut a = pgrounddown(va.into_usize());
        let last = pgrounddown(va.into_usize() + size - 1usize);
        loop {
            let pte = self.walk(VAddr::new(a), true).ok_or(())?;
            assert!(!pte.is_valid(), "remap");

            pte.set_inner(pa2pte(pa) | perm | PTE_V);
            if a == last {
                break;
            }
            a += PGSIZE;
            pa = pa + PGSIZE;
        }
        Ok(())
    }
}

impl PageTable<UVAddr> {
    /// Create a user page table with no user memory,
    /// but with the trampoline and a given trap frame.
    /// Return Some(..) if every allocation has succeeded.
    /// Return None otherwise.
    // TODO(rv6)
    // Change the parameter type.
    // https://github.com/kaist-cp/rv6/issues/338
    pub fn new(trap_frame: *mut Trapframe) -> Option<Self> {
        let mut page_table = Self::new_empty_table()?;

        // Map the trampoline code (for system call return)
        // at the highest user virtual address.
        // Only the supervisor uses it, on the way
        // to/from user space, so not PTE_U.
        if page_table
            .map_pages(
                UVAddr::new(TRAMPOLINE),
                PGSIZE,
                PAddr::new(unsafe { trampoline.as_mut_ptr() as usize }),
                PTE_R | PTE_X,
            )
            .is_err()
        {
            page_table.drop();
            return None;
        }

        // Map the trapframe just below TRAMPOLINE, for trampoline.S.
        if page_table
            .map_pages(
                UVAddr::new(TRAPFRAME),
                PGSIZE,
                PAddr::new(trap_frame as _),
                PTE_R | PTE_W,
            )
            .is_err()
        {
            page_table.unmap(UVAddr::new(TRAMPOLINE), 1, false);
            page_table.drop();
            return None;
        }
        Some(page_table)
    }

    /// Load the user initcode into address 0 of pagetable,
    /// for the very first process.
    /// src.len() must be less than a page.
    pub unsafe fn init(&mut self, src: &[u8]) -> Result<(), ()> {
        assert!(src.len() < PGSIZE, "init: more than a page");

        let page = kernel().alloc().ok_or(())?;
        let mem = page.into_usize() as *mut u8;
        ptr::write_bytes(mem, 0, PGSIZE);
        ptr::copy(src.as_ptr(), mem, src.len());
        self.num_usr_pages = 1;
        self.map_pages(
            VAddr::new(0),
            PGSIZE,
            PAddr::new(mem as usize),
            PTE_W | PTE_R | PTE_X | PTE_U,
        )
    }

    /// Look up a virtual address, return Some(physical address),
    /// or None if not mapped.
    pub fn walk_addr(&mut self, va: UVAddr) -> Option<PAddr> {
        if va.into_usize() >= MAXVA {
            return None;
        }
        let pte = self.walk(va, false)?;
        if !pte.is_user_accessible() {
            return None;
        }
        Some(pte.get_pa())
    }

    /// Allocate PTEs and physical memory to grow process from oldsz to
    /// newsz, which need not be page aligned. Returns Ok(new size) or Err(()) on error.
    pub fn alloc(&mut self, mut oldsz: usize, newsz: usize) -> Result<usize, ()> {
        if newsz < oldsz {
            return Ok(oldsz);
        }
        oldsz = pgroundup(oldsz);
        let mut a = oldsz;
        while a < newsz {
            let mut mem = some_or!(kernel().alloc(), {
                self.dealloc(a, oldsz);
                return Err(());
            });
            mem.write_bytes(0);
            let pa = mem.into_usize();
            if self
                .map_pages(
                    VAddr::new(a),
                    PGSIZE,
                    PAddr::new(pa),
                    PTE_W | PTE_X | PTE_R | PTE_U,
                )
                .is_err()
            {
                // It is safe because pa is an address of mem, which is a page
                // obtained by alloc().
                kernel().free(unsafe { Page::from_usize(pa) });
                self.dealloc(a, oldsz);
                return Err(());
            }
            a += PGSIZE;
            self.num_usr_pages += 1;
        }
        Ok(newsz)
    }

    /// Given a parent process's page table, copy
    /// its memory into a child's page table.
    /// Copies both the page table and the
    /// physical memory.
    /// Returns Ok(()) on success, Err(()) on failure.
    /// Frees any allocated pages on failure.
    pub unsafe fn copy(&mut self, mut new: &mut PageTable<UVAddr>, sz: usize) -> Result<(), ()> {
        for i in num_iter::range_step(0, sz, PGSIZE) {
            let pte = self
                .walk(UVAddr::new(i), false)
                .expect("uvmcopy: pte should exist");
            assert!(pte.is_valid(), "uvmcopy: page not present");

            let mut new_ptable = scopeguard::guard(new, |ptable| {
                ptable.unmap(UVAddr::new(0), i.wrapping_div(PGSIZE), true);
            });
            let pa = pte.get_pa();
            let flags = pte.get_flags();
            let mem = kernel().alloc().ok_or(())?.into_usize();
            ptr::copy(
                pa.into_usize() as *mut u8 as *const u8,
                mem as *mut u8,
                PGSIZE,
            );
            if (*new_ptable)
                .map_pages(VAddr::new(i), PGSIZE, PAddr::new(mem as usize), flags)
                .is_err()
            {
                kernel().free(Page::from_usize(mem as _));
                return Err(());
            }
            new = scopeguard::ScopeGuard::into_inner(new_ptable);
        }
        new.num_usr_pages = self.num_usr_pages;
        Ok(())
    }

    /// Remove npages of mappings starting from va. va must be
    /// page-aligned. The mappings must exist.
    /// Optionally free the physical memory.
    pub fn unmap(&mut self, va: UVAddr, npages: usize, do_free: bool) {
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
    pub fn dealloc(&mut self, oldsz: usize, newsz: usize) -> usize {
        if newsz >= oldsz {
            return oldsz;
        }

        if pgroundup(newsz) < pgroundup(oldsz) {
            let npages = (pgroundup(oldsz).wrapping_sub(pgroundup(newsz))).wrapping_div(PGSIZE);
            self.unmap(UVAddr::new(pgroundup(newsz)), npages, true);
            self.num_usr_pages -= npages;
        }
        newsz
    }

    /// Free user memory pages,
    /// then free page-table pages.
    pub fn free(mut self, sz: usize) {
        if sz > 0 {
            self.unmap(UVAddr::new(0), pgroundup(sz).wrapping_div(PGSIZE), true);
        }
        // It is safe because this method consumes self, so the internal
        // raw page table will not be use anymore.
        unsafe { self.as_inner_mut().free_walk() };
    }

    pub fn drop(&mut self) {
        // Unmap the TRAMPOLINE page.
        if let Some(pte) = self.walk(UVAddr::new(TRAMPOLINE), false) {
            if pte.get_flags() != PTE_V {
                self.unmap(UVAddr::new(TRAMPOLINE), 1, false);
            }
        }
        // Unmap the TRAPFRAME page.
        if let Some(pte) = self.walk(UVAddr::new(TRAPFRAME), false) {
            if pte.get_flags() != PTE_V {
                self.unmap(UVAddr::new(TRAPFRAME), 1, false);
            }
        }
        // Unmap other pages.
        if self.num_usr_pages != 0 {
            self.unmap(UVAddr::new(0), self.num_usr_pages, true);
        }
        unsafe { self.as_inner_mut().free_walk(); }
    }

    /// Mark a PTE invalid for user access.
    /// Used by exec for the user stack guard page.
    pub fn clear(&mut self, va: UVAddr) {
        self.walk(va, false)
            .expect("clear")
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
            let pa0 = self.walk_addr(VAddr::new(va0)).ok_or(())?.into_usize();
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
            let pa0 = self.walk_addr(VAddr::new(va0)).ok_or(())?.into_usize();
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
            let pa0 = self.walk_addr(VAddr::new(va0)).ok_or(())?.into_usize();
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
    pub fn new() -> Option<Self> {
        let mut page_table = Self::new_empty_table()?;

        // SiFive Test Finisher MMIO
        page_table
            .map_pages(
                KVAddr::new(FINISHER),
                PGSIZE,
                PAddr::new(FINISHER),
                PTE_R | PTE_W,
            )
            .ok()?;

        // Uart registers
        page_table
            .map_pages(KVAddr::new(UART0), PGSIZE, PAddr::new(UART0), PTE_R | PTE_W)
            .ok()?;

        // Virtio mmio disk interface
        page_table
            .map_pages(
                KVAddr::new(VIRTIO0),
                PGSIZE,
                PAddr::new(VIRTIO0),
                PTE_R | PTE_W,
            )
            .ok()?;

        // PLIC
        page_table
            .map_pages(KVAddr::new(PLIC), 0x400000, PAddr::new(PLIC), PTE_R | PTE_W)
            .ok()?;

        // Map kernel text executable and read-only.
        let et = unsafe { etext.as_mut_ptr() as usize };
        page_table
            .map_pages(
                KVAddr::new(KERNBASE),
                et - KERNBASE,
                PAddr::new(KERNBASE),
                PTE_R | PTE_X,
            )
            .ok()?;

        // Map kernel data and the physical RAM we'll make use of.
        page_table
            .map_pages(KVAddr::new(et), PHYSTOP - et, PAddr::new(et), PTE_R | PTE_W)
            .ok()?;

        // Map the trampoline for trap entry/exit to
        // the highest virtual address in the kernel.
        page_table
            .map_pages(
                KVAddr::new(TRAMPOLINE),
                PGSIZE,
                PAddr::new(unsafe { trampoline.as_mut_ptr() as usize }),
                PTE_R | PTE_X,
            )
            .ok()?;

        // Allocate a page for the process's kernel stack.
        // Map it high in memory, followed by an invalid
        // guard page.
        for i in 0..NPROC {
            let pa = kernel().alloc()?.into_usize();
            let va: usize = kstack(i);
            page_table
                .map_pages(
                    KVAddr::new(va),
                    PGSIZE,
                    PAddr::new(pa as usize),
                    PTE_R | PTE_W,
                )
                .ok()?;
        }

        Some(page_table)
    }

    /// Switch h/w page table register to the kernel's page table,
    /// and enable paging.
    pub unsafe fn init_hart(&self) {
        w_satp(make_satp(self.ptr as usize));
        sfence_vma();
    }
}
