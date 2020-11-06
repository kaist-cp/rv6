use crate::{
    kernel::kernel,
    memlayout::{CLINT, FINISHER, KERNBASE, PHYSTOP, PLIC, TRAMPOLINE, UART0, VIRTIO0},
    page::{Page, RawPage},
    println,
    proc::myproc,
    riscv::{
        make_satp, pa2pte, pgrounddown, pgroundup, pte2pa, pte_flags, px, sfence_vma, w_satp, PteT,
        MAXVA, PGSIZE, PTE_R, PTE_U, PTE_V, PTE_W, PTE_X,
    },
    some_or,
};
use core::{
    marker::PhantomData,
    mem,
    ops::{Add, Deref, DerefMut},
    ptr,
};

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
            .copyin(dst, src)
            .map_or(Err(()), |_v| Ok(()))
    }

    unsafe fn copyout(dst: Self, src: &[u8]) -> Result<(), ()> {
        let p = myproc();
        (*(*p).data.get())
            .pagetable
            .copyout(dst, src)
            .map_or(Err(()), |_v| Ok(()))
    }
}

#[derive(Default)]
pub struct PageTableEntry {
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

    unsafe fn as_page(&self) -> &RawPage {
        &*(pte2pa(self.inner).into_usize() as *const RawPage)
    }

    fn as_table_mut(&mut self) -> Option<&mut RawPageTable> {
        if self.check_flag(PTE_V) && !self.check_flag((PTE_R | PTE_W | PTE_X) as usize) {
            Some(unsafe { &mut *(pte2pa(self.inner).into_usize() as *mut RawPageTable) })
        } else {
            None
        }
    }

    unsafe fn as_table_mut_unchecked(&mut self) -> &mut RawPageTable {
        &mut *(pte2pa(self.inner).into_usize() as *mut RawPageTable)
    }
}

const PTE_PER_PT: usize = PGSIZE / mem::size_of::<PageTableEntry>();

pub struct RawPageTable {
    inner: [PageTableEntry; PTE_PER_PT],
}

impl Deref for RawPageTable {
    type Target = [PageTableEntry; PTE_PER_PT];
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for RawPageTable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl RawPageTable {
    /// Recursively free page-table pages.
    /// All leaf mappings must already have been removed.
    unsafe fn freewalk(&mut self) {
        // There are 2^9 = 512 PTEs in a page table.
        for pte in &mut self.inner {
            if let Some(ptable) = pte.as_table_mut() {
                ptable.freewalk();
                pte.set_inner(0);
            } else {
                assert!(!pte.check_flag(PTE_V), "freewalk: leaf");
            }
        }
        kernel().free(Page::from_usize(self.as_mut_ptr() as _));
    }
}

pub struct PageTable<A> {
    ptr: *mut RawPageTable,
    _marker: PhantomData<A>,
}

impl<A: VAddr> PageTable<A> {
    pub const fn zero() -> Self {
        Self {
            ptr: ptr::null_mut(),
            _marker: PhantomData,
        }
    }

    pub fn alloc_root(&mut self) {
        let mut page = unsafe { kernel().alloc() }.expect("PageTable new: out of memory");
        page.write_bytes(0);
        self.ptr = page.into_usize() as *mut _;
    }

    pub fn from_raw(ptr: *mut RawPageTable) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    pub fn into_raw(self) -> *mut RawPageTable {
        let ret = self.ptr;
        mem::forget(self);
        ret
    }

    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    pub fn as_raw(&self) -> *mut RawPageTable {
        self.ptr
    }

    /// Return the address of the PTE in page table pagetable
    /// that corresponds to virtual address va. If alloc!=0,
    /// create any required page-table pages.
    ///
    /// The risc-v Sv39 scheme has three levels of page-table
    /// pages. A page-table page contains 512 64-bit PTEs.
    /// A 64-bit virtual address is split into five fields:
    ///   39..63 -- must be zero.
    ///   30..38 -- 9 bits of level-2 index.
    ///   21..39 -- 9 bits of level-1 index.
    ///   12..20 -- 9 bits of level-0 index.
    ///    0..12 -- 12 bits of byte offset within the page.
    unsafe fn walk(&self, va: A, alloc: i32) -> Option<&mut PageTableEntry> {
        let mut pagetable = &mut *self.as_raw();
        assert!(va.into_usize() < MAXVA, "walk");

        for level in (1..3).rev() {
            let pte = &mut pagetable[px(level, va)];
            if pte.check_flag(PTE_V) {
                pagetable = pte.as_table_mut_unchecked();
            } else {
                if alloc == 0 {
                    return None;
                }
                let mut page = kernel().alloc()?;
                page.write_bytes(0);
                let k = page.into_usize();

                pte.set_inner(pa2pte(PAddr::new(k)));
                pte.set_flag(PTE_V);
                pagetable = pte.as_table_mut_unchecked();
            }
        }
        Some(&mut pagetable[px(0, va)])
    }

    /// Look up a virtual address, return the physical address,
    /// or 0 if not mapped.
    pub unsafe fn walkaddr(&mut self, va: A) -> Option<PAddr> {
        if va.into_usize() >= MAXVA {
            return None;
        }
        let pt = self;
        let pte = pt.walk(va, 0)?;
        if !pte.check_flag(PTE_V) {
            return None;
        }
        if !pte.check_flag(PTE_U as usize) {
            return None;
        }
        Some(pte.get_pa())
    }

    /// Create PTEs for virtual addresses starting at va that refer to
    /// physical addresses starting at pa. va and size might not
    /// be page-aligned. Returns Ok(()) on success, Err(()) if walk() couldn't
    /// allocate a needed page-table page.
    pub unsafe fn mappages(
        &mut self,
        va: A,
        size: usize,
        mut pa: usize,
        perm: i32,
    ) -> Result<(), ()> {
        let mut a = pgrounddown(va.into_usize());
        let last = pgrounddown(va.into_usize() + size - 1usize);
        loop {
            let pte = some_or!(self.walk(VAddr::new(a), 1), return Err(()));
            assert!(!pte.check_flag(PTE_V), "remap");

            pte.set_inner(pa2pte(PAddr::new(pa)) | perm as usize | PTE_V);
            if a == last {
                break;
            }
            a += PGSIZE;
            pa += PGSIZE;
        }
        Ok(())
    }

    /// Copy from kernel to user.
    /// Copy len bytes from src to virtual address dstva in a given page table.
    /// Return Ok(()) on success, Err(()) on error.
    pub unsafe fn copyout(&mut self, dstva: UVAddr, src: &[u8]) -> Result<(), ()> {
        let mut dst = dstva.into_usize();
        let mut len = src.len();
        let mut offset = 0;
        while len > 0 {
            let va0 = pgrounddown(dst);
            let pa0 = some_or!(self.walkaddr(VAddr::new(va0)), return Err(())).into_usize();
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
}

impl PageTable<UVAddr> {
    /// Load the user initcode into address 0 of pagetable,
    /// for the very first process.
    /// sz must be less than a page.
    pub unsafe fn uvminit(&mut self, src: &[u8]) {
        assert!(src.len() < PGSIZE, "inituvm: more than a page");

        let mem = kernel().alloc().unwrap().into_usize() as *mut u8;
        ptr::write_bytes(mem, 0, PGSIZE);
        self.mappages(
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
    pub unsafe fn uvmalloc(&mut self, mut oldsz: usize, newsz: usize) -> Result<usize, ()> {
        if newsz < oldsz {
            return Ok(oldsz);
        }
        oldsz = pgroundup(oldsz);
        let mut a = oldsz;
        while a < newsz {
            let mut mem = some_or!(kernel().alloc(), {
                self.uvmdealloc(a, oldsz);
                return Err(());
            });
            mem.write_bytes(0);
            let pa = mem.into_usize();
            if self
                .mappages(VAddr::new(a), PGSIZE, pa, PTE_W | PTE_X | PTE_R | PTE_U)
                .is_err()
            {
                kernel().free(Page::from_usize(pa));
                self.uvmdealloc(a, oldsz);
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
    pub unsafe fn uvmcopy(&mut self, mut new: &mut PageTable<UVAddr>, sz: usize) -> Result<(), ()> {
        for i in num_iter::range_step(0, sz, PGSIZE) {
            let pte = self
                .walk(UVAddr::new(i), 0)
                .expect("uvmcopy: pte should exist");
            assert!(pte.check_flag(PTE_V), "uvmcopy: page not present");

            let mut new_ptable = scopeguard::guard(new, |ptable| {
                ptable.uvmunmap(UVAddr::new(0), i, 1);
            });
            let pa = pte.get_pa();
            let flags = pte.get_flags() as u32;
            let mem = some_or!(kernel().alloc(), return Err(())).into_usize();
            ptr::copy(
                pa.into_usize() as *mut u8 as *const u8,
                mem as *mut u8,
                PGSIZE,
            );
            if (*new_ptable)
                .mappages(VAddr::new(i), PGSIZE, mem as usize, flags as i32)
                .is_err()
            {
                kernel().free(Page::from_usize(mem as _));
                return Err(());
            }
            new = scopeguard::ScopeGuard::into_inner(new_ptable);
        }
        Ok(())
    }

    /// Remove mappings from a page table. The mappings in
    /// the given range must exist. Optionally free the
    /// physical memory.
    pub unsafe fn uvmunmap(&mut self, va: UVAddr, size: usize, do_free: i32) {
        let mut pa: usize = 0;
        let mut a = pgrounddown(va.into_usize());
        let last = pgrounddown(va.into_usize() + size - 1usize);
        loop {
            let pt = &mut *self;
            let pte = pt.walk(UVAddr::new(a), 0).expect("uvmunmap: walk");
            if !pte.check_flag(PTE_V) {
                println!(
                    "va={:018p} pte={:018p}",
                    a as *const u8, pte.inner as *const u8
                );
                panic!("uvmunmap: not mapped");
            }
            assert_ne!(pte.get_flags(), PTE_V, "uvmunmap: not a leaf");

            if do_free != 0 {
                pa = pte.get_pa().into_usize();
                kernel().free(Page::from_usize(pa as _));
            }
            pte.set_inner(0);
            if a == last {
                break;
            }
            a += PGSIZE;
            pa += PGSIZE;
        }
    }

    /// Deallocate user pages to bring the process size from oldsz to
    /// newsz.  oldsz and newsz need not be page-aligned, nor does newsz
    /// need to be less than oldsz.  oldsz can be larger than the actual
    /// process size.  Returns the new process size.
    pub unsafe fn uvmdealloc(&mut self, oldsz: usize, newsz: usize) -> usize {
        if newsz >= oldsz {
            return oldsz;
        }
        let newup: usize = pgroundup(newsz);
        if newup < pgroundup(oldsz) {
            self.uvmunmap(UVAddr::new(newup), oldsz - newup, 1);
        }
        newsz
    }

    /// Free user memory pages,
    /// then free page-table pages.
    pub unsafe fn uvmfree(&mut self, sz: usize) {
        self.uvmunmap(UVAddr::new(0), sz, 1);
        self.freewalk();
    }

    /// Mark a PTE invalid for user access.
    /// Used by exec for the user stack guard page.
    pub unsafe fn uvmclear(&mut self, va: UVAddr) {
        self.walk(va, 0)
            .expect("uvmclear")
            .clear_flag(PTE_U as usize);
    }

    /// Copy from user to kernel.
    /// Copy len bytes to dst from virtual address srcva in a given page table.
    /// Return Ok(()) on success, Err(()) on error.
    pub unsafe fn copyin(&mut self, dst: &mut [u8], srcva: UVAddr) -> Result<(), ()> {
        let mut src = srcva.into_usize();
        let mut len = dst.len();
        let mut offset = 0;
        while len > 0 {
            let va0 = pgrounddown(src);
            let pa0 = some_or!(self.walkaddr(VAddr::new(va0)), return Err(())).into_usize();
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
    pub unsafe fn copyinstr(&mut self, dst: &mut [u8], srcva: UVAddr) -> Result<(), ()> {
        let mut got_null: i32 = 0;
        let mut src = srcva.into_usize();
        let mut offset = 0;
        let mut max = dst.len();
        while got_null == 0 && max > 0 {
            let va0 = pgrounddown(src);
            let pa0 = some_or!(self.walkaddr(VAddr::new(va0)), return Err(())).into_usize();
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

impl<T> Deref for PageTable<T> {
    type Target = RawPageTable;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl<T> DerefMut for PageTable<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.ptr }
    }
}

impl PageTable<KVAddr> {
    // trampoline.S
    /// Create a direct-map page table for the kernel and
    /// turn on paging. Called early, in supervisor mode.
    /// The page allocator is already initialized.
    pub unsafe fn kvminit(&mut self) {
        self.alloc_root();

        // SiFive Test Finisher MMIO
        self.kvmmap(
            KVAddr::new(FINISHER),
            PAddr::new(FINISHER),
            PGSIZE,
            PTE_R | PTE_W,
        );

        // uart registers
        self.kvmmap(KVAddr::new(UART0), PAddr::new(UART0), PGSIZE, PTE_R | PTE_W);

        // virtio mmio disk interface
        self.kvmmap(
            KVAddr::new(VIRTIO0),
            PAddr::new(VIRTIO0),
            PGSIZE,
            PTE_R | PTE_W,
        );

        // CLINT
        self.kvmmap(
            KVAddr::new(CLINT),
            PAddr::new(CLINT),
            0x10000,
            PTE_R | PTE_W,
        );

        // PLIC
        self.kvmmap(KVAddr::new(PLIC), PAddr::new(PLIC), 0x400000, PTE_R | PTE_W);

        // Map kernel text executable and read-only.
        self.kvmmap(
            KVAddr::new(KERNBASE),
            PAddr::new(KERNBASE),
            (etext.as_mut_ptr() as usize) - KERNBASE,
            PTE_R | PTE_X,
        );

        // Map kernel data and the physical RAM we'll make use of.
        self.kvmmap(
            KVAddr::new(etext.as_mut_ptr() as usize),
            PAddr::new(etext.as_mut_ptr() as usize),
            PHYSTOP - (etext.as_mut_ptr() as usize),
            PTE_R | PTE_W,
        );

        // Map the trampoline for trap entry/exit to
        // the highest virtual address in the kernel.
        self.kvmmap(
            KVAddr::new(TRAMPOLINE),
            PAddr::new(trampoline.as_mut_ptr() as usize),
            PGSIZE,
            PTE_R | PTE_X,
        );
    }

    /// Switch h/w page table register to the kernel's page table,
    /// and enable paging.
    pub unsafe fn kvminithart(&self) {
        w_satp(make_satp(self.ptr as usize));
        sfence_vma();
    }

    /// Add a mapping to the kernel page table.
    /// Only used when booting.
    /// Does not flush TLB or enable paging.
    pub unsafe fn kvmmap(&mut self, va: KVAddr, pa: PAddr, sz: usize, perm: i32) {
        self.mappages(va, sz, pa.into_usize(), perm)
            .expect("kvmmap");
    }

    /// Translate a kernel virtual address to
    /// a physical address. Only needed for
    /// addresses on the stack.
    /// Assumes va is page aligned.
    pub unsafe fn kvmpa(&self, va: KVAddr) -> usize {
        let off: usize = va.into_usize().wrapping_rem(PGSIZE);
        let pte = self
            .walk(va, 0)
            .filter(|pte| pte.check_flag(PTE_V))
            .expect("kvmpa");
        let pa = pte.as_page() as *const _ as usize;
        pa + off
    }
}
