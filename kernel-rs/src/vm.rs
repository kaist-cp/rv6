use crate::{
    kalloc::{kalloc, kfree},
    memlayout::{CLINT, KERNBASE, PHYSTOP, PLIC, TRAMPOLINE, UART0, VIRTIO0},
    println,
    riscv::{
        make_satp, pa2pte, pgrounddown, pgroundup, pte2pa, pte_flags, px, sfence_vma, w_satp, PteT,
        MAXVA, PGSIZE, PTE_R, PTE_U, PTE_V, PTE_W, PTE_X,
    },
    some_or,
};
use crate::{libc, page::Page};
use core::{mem, ops::Deref, ops::DerefMut, ptr};

extern "C" {
    // kernel.ld sets this to end of kernel code.
    #[no_mangle]
    static mut etext: [u8; 0];

    // trampoline.S
    #[no_mangle]
    static mut trampoline: [u8; 0];
}

pub struct PageTableEntry {
    inner: PteT,
}

impl PageTableEntry {
    /// Creates a page table entry from the inner representation.
    ///
    /// # Safety
    ///
    /// Improper use of this function may lead to memory problems. For example, a double-free may
    /// occur if the function is called twice on the same raw pointer.
    unsafe fn from_raw(inner: PteT) -> Self {
        Self { inner }
    }

    fn check_flag(&self, flag: usize) -> bool {
        self.inner & flag != 0
    }

    fn set_valid(&mut self, flag: usize) {
        self.inner |= flag;
    }

    fn set_invalid(&mut self, flag: usize) {
        self.inner &= !flag;
    }

    fn set_inner(&mut self, inner: PteT) {
        self.inner = inner;
    }

    fn get_pa(&self) -> usize {
        pte2pa(self.inner)
    }

    fn get_flags(&self) -> usize {
        pte_flags(self.inner)
    }

    unsafe fn as_page(&self) -> &Page {
        &*(pte2pa(self.inner) as *const Page)
    }

    unsafe fn as_table(&self) -> &RawPageTable {
        &*(pte2pa(self.inner) as *const RawPageTable)
    }

    unsafe fn as_table_mut(&mut self) -> &mut RawPageTable {
        &mut *(pte2pa(self.inner) as *mut RawPageTable)
    }
}

const PTSIZE: usize = PGSIZE / mem::size_of::<PageTableEntry>();
pub struct RawPageTable {
    inner: [PageTableEntry; PTSIZE],
}

impl Deref for RawPageTable {
    type Target = [PageTableEntry; PTSIZE];
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for RawPageTable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

pub struct PageTable {
    pub ptr: *mut RawPageTable,
}

impl PageTable {
    pub const unsafe fn raw() -> Self {
        Self {
            ptr: ptr::null_mut(),
        }
    }

    pub fn init(&mut self) {
        let page = unsafe { kalloc() } as *mut RawPageTable;
        if page.is_null() {
            panic!("PageTable init: out of memory");
        }
        unsafe {
            ptr::write_bytes(page as *mut libc::CVoid, 0, PGSIZE);
        }
        self.ptr = page;
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
pub static mut KERNEL_PAGETABLE: PageTable = unsafe { PageTable::raw() };

// trampoline.S
/// Create a direct-map page table for the kernel and
/// turn on paging. Called early, in supervisor mode.
/// The page allocator is already initialized.
pub unsafe fn kvminit() {
    KERNEL_PAGETABLE.init();

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
    w_satp(make_satp(KERNEL_PAGETABLE.ptr as usize));
    sfence_vma();
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
unsafe fn walk(
    mut pagetable: &mut RawPageTable,
    va: usize,
    alloc: i32,
) -> Option<&mut PageTableEntry> {
    if va >= MAXVA {
        panic!("walk");
    }
    for level in (1..3).rev() {
        let pte = &mut pagetable[px(level, va)];
        if pte.check_flag(PTE_V) {
            pagetable = pte.as_table_mut();
        } else {
            if alloc == 0 {
                return None;
            }
            let k = kalloc();
            if k.is_null() {
                return None;
            }

            ptr::write_bytes(k as *mut libc::CVoid, 0, PGSIZE);
            pte.set_inner(pa2pte(k as usize));
            pte.set_valid(PTE_V);
            pagetable = pte.as_table_mut();
        }
    }
    Some(&mut pagetable[px(0, va)])
}

/// Look up a virtual address, return the physical address,
/// or 0 if not mapped.
/// Can only be used to look up user pages.
pub unsafe fn walkaddr(pagetable: &mut RawPageTable, va: usize) -> usize {
    if va >= MAXVA {
        return 0;
    }
    let pte_op = walk(pagetable, va, 0);
    if pte_op.is_none() {
        return 0;
    }
    let pte = pte_op.unwrap();
    if !pte.check_flag(PTE_V) {
        return 0;
    }
    if !pte.check_flag(PTE_U as usize) {
        return 0;
    }
    pte.get_pa()
}

/// Add a mapping to the kernel page table.
/// Only used when booting.
/// Does not flush TLB or enable paging.
pub unsafe fn kvmmap(va: usize, pa: usize, sz: usize, perm: i32) {
    if !mappages(&mut KERNEL_PAGETABLE, va, sz, pa, perm) {
        panic!("kvmmap");
    };
}

/// Translate a kernel virtual address to
/// a physical address. Only needed for
/// addresses on the stack.
/// Assumes va is page aligned.
pub unsafe fn kvmpa(va: usize) -> usize {
    let off: usize = va.wrapping_rem(PGSIZE);
    let pte = walk(&mut KERNEL_PAGETABLE, va, 0)
        .filter(|pte| pte.check_flag(PTE_V))
        .expect("kvmpa");
    let pa = pte.as_page() as *const _ as usize;
    pa.wrapping_add(off)
}

/// Create PTEs for virtual addresses starting at va that refer to
/// physical addresses starting at pa. va and size might not
/// be page-aligned. Returns true on success, false if walk() couldn't
/// allocate a needed page-table page.
pub unsafe fn mappages(
    pagetable: &mut RawPageTable,
    va: usize,
    size: usize,
    mut pa: usize,
    perm: i32,
) -> bool {
    let mut a = pgrounddown(va);
    let last = pgrounddown(va.wrapping_add(size).wrapping_sub(1usize));
    loop {
        let pte = some_or!(walk(pagetable, a, 1), return false);
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
    true
}

/// Remove mappings from a page table. The mappings in
/// the given range must exist. Optionally free the
/// physical memory.
pub unsafe fn uvmunmap(pagetable: &mut RawPageTable, va: usize, size: usize, do_free: i32) {
    let mut pa: usize = 0;
    let mut a = pgrounddown(va);
    let last = pgrounddown(va.wrapping_add(size).wrapping_sub(1usize));
    loop {
        let pte_op = walk(pagetable, a, 0);
        if pte_op.is_none() {
            panic!("uvmunmap: walk");
        }
        let pte = pte_op.unwrap();
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
            kfree(pa as *mut libc::CVoid);
        }
        pte.set_inner(0);
        if a == last {
            break;
        }
        a = a.wrapping_add(PGSIZE);
        pa = pa.wrapping_add(PGSIZE);
    }
}

/// Load the user initcode into address 0 of pagetable,
/// for the very first process.
/// sz must be less than a page.
pub unsafe fn uvminit(pagetable: &mut PageTable, src: *mut u8, sz: u32) {
    if sz >= PGSIZE as u32 {
        panic!("inituvm: more than a page");
    }
    let mem: *mut u8 = kalloc() as *mut u8;
    ptr::write_bytes(mem as *mut libc::CVoid, 0, PGSIZE);
    mappages(
        pagetable,
        0,
        PGSIZE,
        mem as usize,
        PTE_W | PTE_R | PTE_X | PTE_U,
    );
    ptr::copy(
        src as *const libc::CVoid,
        mem as *mut libc::CVoid,
        sz as usize,
    );
}

/// Allocate PTEs and physical memory to grow process from oldsz to
/// newsz, which need not be page aligned.  Returns Some(new size) or None on error.
pub unsafe fn uvmalloc(pagetable: &mut PageTable, mut oldsz: usize, newsz: usize) -> Option<usize> {
    if newsz < oldsz {
        return Some(oldsz);
    }
    oldsz = pgroundup(oldsz);
    let mut a = oldsz;
    while a < newsz {
        let mem = kalloc() as *mut u8;
        if mem.is_null() {
            uvmdealloc(pagetable, a, oldsz);
            return None;
        }
        ptr::write_bytes(mem as *mut libc::CVoid, 0, PGSIZE);
        if !mappages(
            pagetable,
            a,
            PGSIZE,
            mem as usize,
            PTE_W | PTE_X | PTE_R | PTE_U,
        ) {
            kfree(mem as *mut libc::CVoid);
            uvmdealloc(pagetable, a, oldsz);
            return None;
        }
        a = a.wrapping_add(PGSIZE);
    }
    Some(newsz)
}

/// Deallocate user pages to bring the process size from oldsz to
/// newsz.  oldsz and newsz need not be page-aligned, nor does newsz
/// need to be less than oldsz.  oldsz can be larger than the actual
/// process size.  Returns the new process size.
pub unsafe fn uvmdealloc(pagetable: &mut PageTable, oldsz: usize, newsz: usize) -> usize {
    if newsz >= oldsz {
        return oldsz;
    }
    let newup: usize = pgroundup(newsz);
    if newup < pgroundup(oldsz) {
        uvmunmap(pagetable, newup, oldsz.wrapping_sub(newup), 1);
    }
    newsz
}

/// Recursively free page-table pages.
/// All leaf mappings must already have been removed.
unsafe fn freewalk(pagetable: &mut RawPageTable) {
    // There are 2^9 = 512 PTEs in a page table.
    for i in 0..512 {
        let pte = &mut pagetable[i];
        if pte.check_flag(PTE_V) && !pte.check_flag((PTE_R | PTE_W | PTE_X) as usize) {
            // This PTE points to a lower-level page table.
            freewalk(pte.as_table_mut());
            pte.set_inner(0);
        } else if pte.check_flag(PTE_V) {
            panic!("freewalk: leaf");
        }
    }
    kfree(pagetable.as_mut_ptr() as *mut libc::CVoid);
}

/// Free user memory pages,
/// then free page-table pages.
pub unsafe fn uvmfree(pagetable: &mut RawPageTable, sz: usize) {
    uvmunmap(pagetable, 0, sz, 1);
    freewalk(pagetable);
}

/// Given a parent process's page table, copy
/// its memory into a child's page table.
/// Copies both the page table and the
/// physical memory.
/// Returns 0 on success, -1 on failure.
/// Frees any allocated pages on failure.
pub unsafe fn uvmcopy(old: &mut RawPageTable, mut new: &mut RawPageTable, sz: usize) -> i32 {
    for i in num_iter::range_step(0, sz, PGSIZE) {
        let pte_op = walk(old, i, 0);
        if pte_op.is_none() {
            panic!("uvmcopy: pte should exist");
        }
        let pte = pte_op.unwrap();
        if !pte.check_flag(PTE_V) {
            panic!("uvmcopy: page not present");
        }
        let mut new_ptable = scopeguard::guard(new, |ptable| {
            uvmunmap(ptable, 0, i, 1);
        });
        let pa = pte.get_pa();
        let flags = pte.get_flags() as u32;
        let mem = kalloc() as *mut u8;
        if mem.is_null() {
            return -1;
        }
        ptr::copy(
            pa as *mut u8 as *const libc::CVoid,
            mem as *mut libc::CVoid,
            PGSIZE,
        );
        if !mappages(*new_ptable, i, PGSIZE, mem as usize, flags as i32) {
            kfree(mem as *mut libc::CVoid);
            return -1;
        }
        new = scopeguard::ScopeGuard::into_inner(new_ptable);
    }

    0
}

/// Mark a PTE invalid for user access.
/// Used by exec for the user stack guard page.
pub unsafe fn uvmclear(pagetable: &mut RawPageTable, va: usize) {
    let pte_op = walk(pagetable, va, 0);
    if pte_op.is_none() {
        panic!("uvmclear");
    }
    pte_op.unwrap().set_invalid(PTE_U as usize)
}

/// Copy from kernel to user.
/// Copy len bytes from src to virtual address dstva in a given page table.
/// Return 0 on success, -1 on error.
pub unsafe fn copyout(
    pagetable: &mut RawPageTable,
    mut dstva: usize,
    mut src: *mut u8,
    mut len: usize,
) -> i32 {
    while len > 0 {
        let va0 = pgrounddown(dstva);
        let pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 {
            return -1;
        }
        let mut n = PGSIZE.wrapping_sub(dstva.wrapping_sub(va0));
        if n > len {
            n = len
        }
        ptr::copy(
            src as *const libc::CVoid,
            pa0.wrapping_add(dstva.wrapping_sub(va0)) as *mut libc::CVoid,
            n,
        );
        len = len.wrapping_sub(n);
        src = src.add(n);
        dstva = va0.wrapping_add(PGSIZE);
    }
    0
}

/// Copy from user to kernel.
/// Copy len bytes to dst from virtual address srcva in a given page table.
/// Return 0 on success, -1 on error.
pub unsafe fn copyin(
    pagetable: &mut RawPageTable,
    mut dst: *mut u8,
    mut srcva: usize,
    mut len: usize,
) -> i32 {
    while len > 0 {
        let va0 = pgrounddown(srcva);
        let pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 {
            return -1;
        }
        let mut n = PGSIZE.wrapping_sub(srcva.wrapping_sub(va0));
        if n > len {
            n = len
        }
        ptr::copy(
            pa0.wrapping_add(srcva.wrapping_sub(va0)) as *mut libc::CVoid,
            dst as *mut libc::CVoid,
            n,
        );
        len = len.wrapping_sub(n);
        dst = dst.add(n);
        srcva = va0.wrapping_add(PGSIZE)
    }
    0
}

/// Copy a null-terminated string from user to kernel.
/// Copy bytes to dst from virtual address srcva in a given page table,
/// until a '\0', or max.
/// Return 0 on success, -1 on error.
pub unsafe fn copyinstr(
    pagetable: &mut RawPageTable,
    mut dst: *mut u8,
    mut srcva: usize,
    mut max: usize,
) -> i32 {
    let mut got_null: i32 = 0;
    while got_null == 0 && max > 0 {
        let va0 = pgrounddown(srcva);
        let pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 {
            return -1;
        }
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
        0
    } else {
        -1
    }
}
