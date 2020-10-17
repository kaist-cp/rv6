use mem::MaybeUninit;

use crate::{
    kalloc::{kalloc, kfree},
    memlayout::{CLINT, FINISHER, KERNBASE, PHYSTOP, PLIC, TRAMPOLINE, UART0, VIRTIO0},
    page::Page,
    println,
    riscv::{
        make_satp, pa2pte, pgrounddown, pgroundup, pte2pa, pte_flags, px, sfence_vma, w_satp, PteT,
        MAXVA, PGSIZE, PTE_R, PTE_U, PTE_V, PTE_W, PTE_X,
    },
    some_or,
};
use core::{mem, ops::Deref, ops::DerefMut, ptr};

extern "C" {
    // kernel.ld sets this to end of kernel code.
    #[no_mangle]
    static mut etext: [u8; 0];

    // trampoline.S
    #[no_mangle]
    static mut trampoline: [u8; 0];
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

    fn get_pa(&self) -> usize {
        pte2pa(self.inner)
    }

    unsafe fn as_page(&self) -> &Page {
        &*(pte2pa(self.inner) as *const Page)
    }

    fn as_table_mut(&mut self) -> Option<&mut RawPageTable> {
        if self.check_flag(PTE_V) && !self.check_flag((PTE_R | PTE_W | PTE_X) as usize) {
            Some(unsafe { &mut *(pte2pa(self.inner) as *mut RawPageTable) })
        } else {
            None
        }
    }

    unsafe fn as_table_mut_unchecked(&mut self) -> &mut RawPageTable {
        &mut *(pte2pa(self.inner) as *mut RawPageTable)
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
    /// Look up a virtual address, return the physical address,
    /// or 0 if not mapped.
    /// TODO: Use type parameter at PageTable to show this function "Can only be used to look up user pages."
    pub unsafe fn walkaddr(&mut self, va: usize) -> Option<usize> {
        if va >= MAXVA {
            return None;
        }
        let mut pt = PageTable::from_raw(self);
        let pte = pt.walk(va, 0)?;
        if !pte.check_flag(PTE_V) {
            return None;
        }
        if !pte.check_flag(PTE_U as usize) {
            return None;
        }
        Some(pte.get_pa())
    }

    /// Recursively free page-table pages.
    /// All leaf mappings must already have been removed.
    unsafe fn freewalk(&mut self) {
        // There are 2^9 = 512 PTEs in a page table.
        for pte in &mut self.inner {
            if pte.check_flag(PTE_V) && !pte.check_flag((PTE_R | PTE_W | PTE_X) as usize) {
                // This PTE points to a lower-level page table.
                pte.as_table_mut_unchecked().freewalk();
                pte.set_inner(0);
            } else if pte.check_flag(PTE_V) {
                panic!("freewalk: leaf");
            }
        }
        kfree(self.as_mut_ptr() as _);
    }

    /// Copy from kernel to user.
    /// Copy len bytes from src to virtual address dstva in a given page table.
    /// Return Ok(()) on success, Err(()) on error.
    // TODO: Refactor src to type &[u8]
    pub unsafe fn copyout(
        &mut self,
        mut dstva: usize,
        mut src: *mut u8,
        mut len: usize,
    ) -> Result<(), ()> {
        while len > 0 {
            let va0 = pgrounddown(dstva);
            let pa0 = some_or!(self.walkaddr(va0), return Err(()));
            let mut n = PGSIZE.wrapping_sub(dstva.wrapping_sub(va0));
            if n > len {
                n = len
            }
            ptr::copy(
                src as *const u8,
                pa0.wrapping_add(dstva.wrapping_sub(va0)) as *mut u8,
                n,
            );
            len = len.wrapping_sub(n);
            src = src.add(n);
            dstva = va0.wrapping_add(PGSIZE);
        }
        Ok(())
    }
}

// TODO: separate these methods for uvm type/struct (Use type to show this)
impl RawPageTable {
    /// Remove mappings from a page table. The mappings in
    /// the given range must exist. Optionally free the
    /// physical memory.
    pub unsafe fn uvmunmap(&mut self, va: usize, size: usize, do_free: i32) {
        let mut pa: usize = 0;
        let mut a = pgrounddown(va);
        let last = pgrounddown(va.wrapping_add(size).wrapping_sub(1usize));
        loop {
            let mut pt = PageTable::from_raw(self);
            let pte = pt.walk(a, 0).expect("uvmunmap: walk");
            if !pte.check_flag(PTE_V) {
                println!(
                    "va={:018p} pte={:018p}",
                    a as *const u8, pte.inner as *const u8
                );
                panic!("uvmunmap: not mapped");
            }
            if pte.get_flags() == PTE_V {
                panic!("uvmunmap: not a leaf");
            }
            if do_free != 0 {
                pa = pte.get_pa();
                kfree(pa as _);
            }
            pte.set_inner(0);
            if a == last {
                break;
            }
            a = a.wrapping_add(PGSIZE);
            pa = pa.wrapping_add(PGSIZE);
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
            self.uvmunmap(newup, oldsz.wrapping_sub(newup), 1);
        }
        newsz
    }

    /// Free user memory pages,
    /// then free page-table pages.
    pub unsafe fn uvmfree(&mut self, sz: usize) {
        self.uvmunmap(0, sz, 1);
        self.freewalk();
    }

    /// Mark a PTE invalid for user access.
    /// Used by exec for the user stack guard page.
    pub unsafe fn uvmclear(&mut self, va: usize) {
        PageTable::from_raw(self)
            .walk(va, 0)
            .expect("uvmclear")
            .clear_flag(PTE_U as usize);
    }

    /// Copy from user to kernel.
    /// Copy len bytes to dst from virtual address srcva in a given page table.
    /// Return Ok(()) on success, Err(()) on error.
    pub unsafe fn copyin(
        &mut self,
        mut dst: *mut u8,
        mut srcva: usize,
        mut len: usize,
    ) -> Result<(), ()> {
        while len > 0 {
            let va0 = pgrounddown(srcva);
            let pa0 = some_or!(self.walkaddr(va0), return Err(()));
            let mut n = PGSIZE.wrapping_sub(srcva.wrapping_sub(va0));
            if n > len {
                n = len
            }
            ptr::copy(pa0.wrapping_add(srcva.wrapping_sub(va0)) as *mut u8, dst, n);
            len = len.wrapping_sub(n);
            dst = dst.add(n);
            srcva = va0.wrapping_add(PGSIZE)
        }
        Ok(())
    }

    /// Copy a null-terminated string from user to kernel.
    /// Copy bytes to dst from virtual address srcva in a given page table,
    /// until a '\0', or max.
    /// Return OK(()) on success, Err(()) on error.
    pub unsafe fn copyinstr(
        &mut self,
        mut dst: *mut u8,
        mut srcva: usize,
        mut max: usize,
    ) -> Result<(), ()> {
        let mut got_null: i32 = 0;
        while got_null == 0 && max > 0 {
            let va0 = pgrounddown(srcva);
            let pa0 = some_or!(self.walkaddr(va0), return Err(()));
            let mut n = PGSIZE.wrapping_sub(srcva.wrapping_sub(va0));
            if n > max {
                n = max
            }
            let mut p: *mut u8 = pa0.wrapping_add(srcva.wrapping_sub(va0)) as *mut u8;
            while n > 0 {
                if *p as i32 == '\u{0}' as i32 {
                    *dst = '\u{0}' as i32 as u8;
                    got_null = 1;
                    break;
                } else {
                    *dst = *p;
                    n = n.wrapping_sub(1);
                    max = max.wrapping_sub(1);
                    p = p.offset(1);
                    dst = dst.offset(1)
                }
            }
            srcva = va0.wrapping_add(PGSIZE)
        }
        if got_null != 0 {
            Ok(())
        } else {
            Err(())
        }
    }
}

pub struct PageTable {
    ptr: *mut RawPageTable,
}

impl PageTable {
    pub fn new() -> Self {
        let page = unsafe { kalloc() } as *mut RawPageTable;
        if page.is_null() {
            panic!("PageTable new: out of memory");
        }
        unsafe {
            ptr::write_bytes(page, 0, 1);
        }

        Self { ptr: page }
    }

    pub fn from_raw(ptr: *mut RawPageTable) -> Self {
        Self { ptr }
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
    unsafe fn walk(&mut self, va: usize, alloc: i32) -> Option<&mut PageTableEntry> {
        let mut pagetable = &mut *self.as_raw();
        if va >= MAXVA {
            panic!("walk");
        }
        for level in (1..3).rev() {
            let pte = &mut pagetable[px(level, va)];
            if pte.check_flag(PTE_V) {
                pagetable = pte.as_table_mut_unchecked();
            } else {
                if alloc == 0 {
                    return None;
                }
                let k = kalloc();
                if k.is_null() {
                    return None;
                }

                ptr::write_bytes(k, 0, PGSIZE);
                pte.set_inner(pa2pte(k as usize));
                pte.set_flag(PTE_V);
                pagetable = pte.as_table_mut_unchecked();
            }
        }
        Some(&mut pagetable[px(0, va)])
    }
}

// TODO: separate these function for va structure later (Use type to show this)
impl PageTable {
    /// Create PTEs for virtual addresses starting at va that refer to
    /// physical addresses starting at pa. va and size might not
    /// be page-aligned. Returns Ok(()) on success, Err(()) if walk() couldn't
    /// allocate a needed page-table page.
    pub unsafe fn mappages(
        &mut self,
        va: usize,
        size: usize,
        mut pa: usize,
        perm: i32,
    ) -> Result<(), ()> {
        let mut a = pgrounddown(va);
        let last = pgrounddown(va.wrapping_add(size).wrapping_sub(1usize));
        loop {
            let pte = some_or!(self.walk(a, 1), return Err(()));
            if pte.check_flag(PTE_V) {
                panic!("remap");
            }
            pte.set_inner(pa2pte(pa) | perm as usize | PTE_V);
            if a == last {
                break;
            }
            a = a.wrapping_add(PGSIZE);
            pa = pa.wrapping_add(PGSIZE);
        }
        Ok(())
    }

    /// Load the user initcode into address 0 of pagetable,
    /// for the very first process.
    /// sz must be less than a page.
    pub unsafe fn uvminit(&mut self, src: *mut u8, sz: u32) {
        if sz >= PGSIZE as u32 {
            panic!("inituvm: more than a page");
        }
        let mem: *mut u8 = kalloc();
        ptr::write_bytes(mem, 0, PGSIZE);
        self.mappages(0, PGSIZE, mem as usize, PTE_W | PTE_R | PTE_X | PTE_U)
            .expect("inituvm: mappage");
        ptr::copy(src as *const u8, mem, sz as usize);
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
            let mem = kalloc();
            if mem.is_null() {
                self.uvmdealloc(a, oldsz);
                return Err(());
            }
            ptr::write_bytes(mem, 0, PGSIZE);
            if self
                .mappages(a, PGSIZE, mem as usize, PTE_W | PTE_X | PTE_R | PTE_U)
                .is_err()
            {
                kfree(mem);
                self.uvmdealloc(a, oldsz);
                return Err(());
            }
            a = a.wrapping_add(PGSIZE);
        }
        Ok(newsz)
    }

    /// Given a parent process's page table, copy
    /// its memory into a child's page table.
    /// Copies both the page table and the
    /// physical memory.
    /// Returns Ok(()) on success, Err(()) on failure.
    /// Frees any allocated pages on failure.
    pub unsafe fn uvmcopy(&mut self, mut new: &mut PageTable, sz: usize) -> Result<(), ()> {
        for i in num_iter::range_step(0, sz, PGSIZE) {
            let pte = self.walk(i, 0).expect("uvmcopy: pte should exist");
            if !pte.check_flag(PTE_V) {
                panic!("uvmcopy: page not present");
            }
            let mut new_ptable = scopeguard::guard(new, |ptable| {
                ptable.uvmunmap(0, i, 1);
            });
            let pa = pte.get_pa();
            let flags = pte.get_flags() as u32;
            let mem = kalloc();
            if mem.is_null() {
                return Err(());
            }
            ptr::copy(pa as *mut u8 as *const u8, mem, PGSIZE);
            if (*new_ptable)
                .mappages(i, PGSIZE, mem as usize, flags as i32)
                .is_err()
            {
                kfree(mem);
                return Err(());
            }
            new = scopeguard::ScopeGuard::into_inner(new_ptable);
        }
        Ok(())
    }
}

impl Deref for PageTable {
    type Target = RawPageTable;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl DerefMut for PageTable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.ptr }
    }
}

///
/// The kernel's page table.
///
pub static mut KERNEL_PAGETABLE: MaybeUninit<PageTable> = MaybeUninit::uninit();

// trampoline.S
/// Create a direct-map page table for the kernel and
/// turn on paging. Called early, in supervisor mode.
/// The page allocator is already initialized.
pub unsafe fn kvminit() {
    // TODO: make MemoryManager which initiate and own KERNEL_PAGETABLE
    KERNEL_PAGETABLE.write(PageTable::new());

    // SiFive Test Finisher MMIO
    kvmmap(FINISHER, FINISHER, PGSIZE, PTE_R | PTE_W);

    // uart registers
    kvmmap(UART0, UART0, PGSIZE, PTE_R | PTE_W);

    // virtio mmio disk interface
    kvmmap(VIRTIO0, VIRTIO0, PGSIZE, PTE_R | PTE_W);

    // CLINT
    kvmmap(CLINT, CLINT, 0x10000, PTE_R | PTE_W);

    // PLIC
    kvmmap(PLIC, PLIC, 0x400000, PTE_R | PTE_W);

    // Map kernel text executable and read-only.
    kvmmap(
        KERNBASE,
        KERNBASE,
        (etext.as_mut_ptr() as usize).wrapping_sub(KERNBASE),
        PTE_R | PTE_X,
    );

    // Map kernel data and the physical RAM we'll make use of.
    kvmmap(
        etext.as_mut_ptr() as usize,
        etext.as_mut_ptr() as usize,
        (PHYSTOP).wrapping_sub(etext.as_mut_ptr() as usize),
        PTE_R | PTE_W,
    );

    // Map the trampoline for trap entry/exit to
    // the highest virtual address in the kernel.
    kvmmap(
        TRAMPOLINE,
        trampoline.as_mut_ptr() as usize,
        PGSIZE,
        PTE_R | PTE_X,
    );
}

/// Switch h/w page table register to the kernel's page table,
/// and enable paging.
pub unsafe fn kvminithart() {
    w_satp(make_satp(KERNEL_PAGETABLE.assume_init_mut().ptr as usize));
    sfence_vma();
}

/// Add a mapping to the kernel page table.
/// Only used when booting.
/// Does not flush TLB or enable paging.
pub unsafe fn kvmmap(va: usize, pa: usize, sz: usize, perm: i32) {
    if KERNEL_PAGETABLE
        .assume_init_mut()
        .mappages(va, sz, pa, perm)
        .is_err()
    {
        panic!("kvmmap");
    };
}

/// Translate a kernel virtual address to
/// a physical address. Only needed for
/// addresses on the stack.
/// Assumes va is page aligned.
pub unsafe fn kvmpa(va: usize) -> usize {
    let off: usize = va.wrapping_rem(PGSIZE);
    let pte = KERNEL_PAGETABLE
        .assume_init_mut()
        .walk(va, 0)
        .filter(|pte| pte.check_flag(PTE_V))
        .expect("kvmpa");
    let pa = pte.as_page() as *const _ as usize;
    pa.wrapping_add(off)
}
