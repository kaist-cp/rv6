use core::{cmp, marker::PhantomData, mem, ops::DerefMut, pin::Pin, slice};

use bitflags::bitflags;
use zerocopy::{AsBytes, FromBytes};

use crate::{
    addr::{pgrounddown, pgroundup, Addr, KVAddr, PAddr, UVAddr, VAddr, MAXVA, PGSIZE},
    arch::interface::{Arch, IPageTableEntry, PageTableManager},
    arch::TargetArch,
    fs::{DefaultFs, InodeGuard},
    kalloc::Kmem,
    lock::SpinLock,
    memlayout::{kstack, PHYSTOP, TRAMPOLINE, TRAPFRAME},
    page::Page,
    param::NPROC,
    proc::KernelCtx,
    util::memmove,
};

type PageTableEntry = <TargetArch as PageTableManager>::PageTableEntry;
type PteFlags = <PageTableEntry as IPageTableEntry>::EntryFlags;

extern "C" {
    // kernel.ld sets this to end of kernel code.
    static mut etext: [u8; 0];

    static mut trampoline: [u8; 0];
}

bitflags! {
    /// Abstraction of access permissions
    pub struct AccessFlags: usize {
        /// readable
        const R = 1 << 0;
        /// writable
        const W = 1 << 1;
        /// executable
        const X = 1 << 2;
        /// user-accessible
        const U = 1 << 3;

        const RW = Self::R.bits | Self::W.bits;
        const RU = Self::R.bits | Self::U.bits;
        const RX = Self::R.bits | Self::X.bits;
        const RWX = Self::RW.bits | Self::X.bits;
        const RWU = Self::RW.bits | Self::U.bits;
        const RXU = Self::RX.bits | Self::U.bits;
        const RWXU = Self::RWX.bits | Self::U.bits;
    }
}

const PTE_PER_PT: usize = PGSIZE / mem::size_of::<PageTableEntry>();

/// # Safety
///
/// It should be converted to a Page by Page::from_usize(self.inner.as_ptr() as _)
/// without breaking the invariants of Page.
pub struct RawPageTable {
    inner: [PageTableEntry; PTE_PER_PT],
}

impl RawPageTable {
    /// Make a new emtpy raw page table by allocating a new page.
    /// Return `Ok(..)` if the allocation has succeeded.
    /// Return `None` if the allocation has failed.
    fn new(allocator: Pin<&SpinLock<Kmem>>) -> Option<*mut RawPageTable> {
        let page = allocator.alloc(Some(0))?;
        // This line guarantees the invariant.
        Some(page.into_usize() as *mut RawPageTable)
    }

    /// Return `Some(..)` if the `index`th entry refers to a page-table page.
    /// Return `Some(..)` by allocating a new page if the `index`th
    /// entry is invalid but `allocator` is `Some`. The result becomes `None` when the
    /// allocation has failed.
    /// Return `None` if the `index`th entry refers to a data page.
    /// Return `None` if the `index`th entry is invalid and `allocator` is `None`.
    fn get_table_mut(
        &mut self,
        index: usize,
        allocator: Option<Pin<&SpinLock<Kmem>>>,
    ) -> Option<&mut RawPageTable> {
        let pte = &mut self.inner[index];
        if !pte.is_valid() {
            let table = Self::new(allocator?)?;
            pte.set_table(table);
        }
        pte.as_table_mut()
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
    ///
    /// # Safety
    ///
    /// This method frees the page table itself, so this page table must
    /// not be used after an invocation of this method.
    unsafe fn free_walk(&mut self, allocator: Pin<&SpinLock<Kmem>>) {
        // There are 2^9 = 512 PTEs in a page table.
        for pte in &mut self.inner {
            if let Some(ptable) = pte.as_table_mut() {
                // SAFETY: ptable will not be used anymore.
                unsafe { ptable.free_walk(allocator) };
                pte.invalidate();
            }
        }
        // SAFETY: safe to convert inner to a Page because of the invariant.
        let page = unsafe { Page::from_usize(self.inner.as_ptr() as _) };
        allocator.free(page);
    }
}

/// # Safety
///
/// ptr uniquely refers to a valid 3-level RawPageTable.
pub struct PageTable<A: VAddr> {
    ptr: *mut RawPageTable,
    _marker: PhantomData<A>,
}

impl<A: VAddr> PageTable<A> {
    /// Make a new empty page table by allocating a new page.
    /// Return `Ok(..)` if the allocation has succeeded.
    /// Return `None` if the allocation has failed.
    fn new(allocator: Pin<&SpinLock<Kmem>>) -> Option<Self> {
        Some(Self {
            ptr: RawPageTable::new(allocator)?,
            _marker: PhantomData,
        })
    }

    fn as_usize(&self) -> usize {
        self.ptr as usize
    }

    /// Return the reference of the PTE in this page table
    /// that corresponds to virtual address `va`. If `allocator` is `Some`,
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
    fn get_mut(
        &mut self,
        va: A,
        allocator: Option<Pin<&SpinLock<Kmem>>>,
    ) -> Option<&mut PageTableEntry> {
        assert!(va.into_usize() < MAXVA, "PageTable::get_mut");
        // SAFETY: self.ptr uniquely refers to a valid RawPageTable
        // according to the invariant.
        let mut page_table = unsafe { &mut *self.ptr };
        for level in (1..3).rev() {
            page_table = page_table.get_table_mut(va.page_table_index(level), allocator)?;
        }
        Some(page_table.get_entry_mut(va.page_table_index(0)))
    }

    pub fn insert(
        &mut self,
        va: A,
        pa: PAddr,
        perm: PteFlags,
        allocator: Pin<&SpinLock<Kmem>>,
    ) -> Result<(), ()> {
        let a = pgrounddown(va.into_usize());
        let pte = self.get_mut(A::from(a), Some(allocator)).ok_or(())?;
        assert!(!pte.is_valid(), "PageTable::insert");
        pte.set_entry(pa, perm);
        Ok(())
    }

    /// Create PTEs for virtual addresses starting at va that refer to
    /// physical addresses starting at pa. va and size might not
    /// be page-aligned. Returns Ok(()) on success, Err(()) if walk() couldn't
    /// allocate a needed page-table page.
    pub fn insert_range(
        &mut self,
        va: A,
        size: usize,
        pa: PAddr,
        perm: PteFlags,
        allocator: Pin<&SpinLock<Kmem>>,
    ) -> Result<(), ()> {
        let start = pgrounddown(va.into_usize());
        let end = pgrounddown(va.into_usize() + size - 1usize);
        for i in num_iter::range_step_inclusive(0, end - start, PGSIZE) {
            self.insert(va + i, pa + i, perm, allocator)?;
        }
        Ok(())
    }

    fn remove(&mut self, va: A) -> Option<PAddr> {
        let pte = self.get_mut(va, None)?;
        assert!(pte.is_data(), "PageTable::remove");
        let pa = pte.get_pa();
        pte.invalidate();
        Some(pa)
    }

    // # Safety
    //
    // This page table must not be used after invoking this method.
    unsafe fn free(&mut self, allocator: Pin<&SpinLock<Kmem>>) {
        // SAFETY:
        // * self.ptr is a valid pointer.
        // * this page table is being dropped, and its ptr will not be used anymore.
        unsafe { (*self.ptr).free_walk(allocator) };
    }
}

impl<A: VAddr> Drop for PageTable<A> {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("PageTable must never drop.");
    }
}

/// UserMemory manages the page table and allocated pages of a process. Its
/// invariant guarantees that every PAddr mapped to VAddr except TRAMPOLINE and
/// TRAPFRAME is from Page. This property is crucial for safety of methods that
/// read or write on memory, such as copy_in. Also, it is essential for safety
/// of freeing a page created from each PAddr as well.
///
/// # Safety
///
/// For brevity, pt := page_table, and we treat pt as a function from va to pa.
/// - If va ∈ dom(pt), va mod PGSIZE = 0 ∧ pt(va) mod PGSIZE = 0.
/// - pt(TRAMPOLINE) = trampoline.
/// - TRAPFRAME ∈ dom(pt).
/// - If va ∈ dom(pt) ∧ va ∉ { TRAMPOLINE, TRAPFRAME },
///   then Page::from_usize(pt(va)) succeeds without breaking the invariant of Page.
/// - If va ∈ dom(pt) where va ∉ { 0, TRAMPOLINE, TRAPFRAME },
///   then va - PGSIZE ∈ dom(pt).
/// - pgroundup(size) ∉ dom(pt).
/// - If size > 0, then pgroundup(size) - PGSIZE ∈ dom(pt).
pub struct UserMemory {
    /// Page table of process.
    page_table: PageTable<UVAddr>,
    /// Size of process memory (bytes).
    size: usize,
}

impl UserMemory {
    /// Create a user page table with no user memory, but with the trampoline
    /// and a given trap frame. If `src_opt` is `Some(src)`, then load `src`
    /// into address 0 of the pagetable. In this case, src.len() must be less
    /// than a page.
    /// Return Some(..) if every allocation has succeeded.
    /// Return None otherwise.
    pub fn new(
        trap_frame: PAddr,
        src_opt: Option<&[u8]>,
        allocator: Pin<&SpinLock<Kmem>>,
    ) -> Option<Self> {
        let page_table = PageTable::new(allocator)?;
        let mut page_table = scopeguard::guard(page_table, |mut page_table| {
            unsafe { page_table.free(allocator) };
            mem::forget(page_table);
        });

        // Map the trampoline code (for system call return)
        // at the highest user virtual address.
        // Only the supervisor uses it, on the way
        // to/from user space, so not PTE_U.
        page_table
            .insert(
                TRAMPOLINE.into(),
                // SAFETY: we assume that reading the address of trampoline is safe.
                (unsafe { trampoline.as_mut_ptr() as usize }).into(),
                (AccessFlags::R | AccessFlags::X).into(),
                allocator,
            )
            .ok()?;

        // Map the trapframe just below TRAMPOLINE, for trampoline.S.
        page_table
            .insert(
                TRAPFRAME.into(),
                trap_frame,
                (AccessFlags::R | AccessFlags::W).into(),
                allocator,
            )
            .ok()?;

        let mut memory = Self {
            page_table: scopeguard::ScopeGuard::into_inner(page_table),
            size: 0,
        };

        if let Some(src) = src_opt {
            assert!(src.len() < PGSIZE, "new: more than a page");
            let mut page = allocator.alloc(Some(0))?;
            memmove(&mut page[..src.len()], src);
            memory
                .push_page(
                    page,
                    (AccessFlags::R | AccessFlags::W | AccessFlags::X | AccessFlags::U).into(),
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
            let mut page = allocator.alloc(None)?;
            // SAFETY: pa is an address in page_table,
            // and thus it is the address of a page by the invariant.
            let src = unsafe { slice::from_raw_parts(pa.into_usize() as *const u8, PGSIZE) };
            memmove(page.deref_mut().deref_mut(), src);

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
        while pgroundup(this.size) < pgroundup(newsz) {
            let page = allocator.alloc(Some(0)).ok_or(())?;
            this.push_page(
                page,
                (AccessFlags::R | AccessFlags::W | AccessFlags::X | AccessFlags::U).into(),
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
            memmove(&mut page[poffset..poffset + n], &src[offset..offset + n]);
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
            memmove(&mut dst[offset..offset + n], &page[poffset..poffset + n]);
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
                    memmove(&mut dst[offset..offset + i + 1], &from[..i + 1]);
                    return Ok(());
                }
                None => {
                    memmove(&mut dst[offset..offset + n], from);
                    max -= n;
                    offset += n;
                    src += n;
                }
            }
        }
        Err(())
    }

    /// Return the address of the page table
    pub fn page_table_addr(&self) -> usize {
        self.page_table.as_usize()
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

    /// Increase the size by appending a given page with given flags.
    /// Ok(()) on success, Err(given page) on failure.
    fn push_page(
        &mut self,
        page: Page,
        perm: PteFlags,
        allocator: Pin<&SpinLock<Kmem>>,
    ) -> Result<(), Page> {
        let pa = page.into_usize();
        // The invariant is maintained because page.addr() is the address of a page.
        let size = pgroundup(self.size);
        self.page_table
            .insert(size.into(), pa.into(), perm, allocator)
            // SAFETY: pa is the address of a given page.
            .map_err(|_| unsafe { Page::from_usize(pa) })?;
        self.size = size + PGSIZE;
        Ok(())
    }

    /// Decrease the size by removing the most recently appended page.
    /// Some(page) if size > 0, None if size = 0.
    fn pop_page(&mut self) -> Option<Page> {
        if self.size == 0 {
            return None;
        }
        self.size = pgroundup(self.size) - PGSIZE;
        let pa = self
            .page_table
            .remove(self.size.into())
            .expect("pop_page")
            .into_usize();
        // SAFETY: pa is an address in page_table,
        // and, thus, it is the address of a page by the invariant.
        Some(unsafe { Page::from_usize(pa) })
    }

    pub fn free(mut self, allocator: Pin<&SpinLock<Kmem>>) {
        let _ = self.dealloc(0, allocator);
        // SAFETY: self will be dropped.
        unsafe { self.page_table.free(allocator) };
        mem::forget(self);
    }
}

impl Drop for UserMemory {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("UserMemory must never drop.");
    }
}

/// KernelMemory manages the page table and allocated pages of the kernel.
/// Every PAddr in KernelMemory is not originated from a page. KernelMemory
/// neither provides memory read/write methods nor decreases memory. Therefore,
/// it does not need an invariant like UserMemory.
// If we modify KernelMemory to extend the kernel in the future, its behavior
// may change, and it may need some invariant. At that moment, we can consider
// what would be the proper invariant for KernelMemory and whether we can
// combine UserMemory and KernelMemory to form a single type.
pub struct KernelMemory<A> {
    /// Page table of kernel.
    page_table: PageTable<KVAddr>,
    _marker: PhantomData<A>,
}

impl<A: Arch> KernelMemory<A> {
    /// Make a direct-map page table for the kernel.
    pub fn new(allocator: Pin<&SpinLock<Kmem>>) -> Option<Self> {
        let page_table = PageTable::new(allocator)?;
        let mut page_table = scopeguard::guard(page_table, |mut page_table| {
            unsafe { page_table.free(allocator) };
            mem::forget(page_table);
        });

        for (start, range) in A::kernel_page_dev_mappings() {
            page_table
                .insert_range(
                    (*start).into(),
                    *range,
                    (*start).into(),
                    (AccessFlags::R | AccessFlags::W).into(),
                    allocator,
                )
                .ok()?;
        }

        // Uart registers
        page_table
            .insert_range(
                A::UART0.into(),
                PGSIZE,
                A::UART0.into(),
                (AccessFlags::R | AccessFlags::W).into(),
                allocator,
            )
            .ok()?;

        // Virtio mmio disk interface
        page_table
            .insert_range(
                A::VIRTIO0.into(),
                PGSIZE,
                A::VIRTIO0.into(),
                (AccessFlags::R | AccessFlags::W).into(),
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
                (AccessFlags::R | AccessFlags::X).into(),
                allocator,
            )
            .ok()?;

        // Map kernel text executable and read-only.
        // SAFETY: we assume that reading the address of etext is safe.
        let et = unsafe { etext.as_mut_ptr() as usize };
        page_table
            .insert_range(
                A::KERNBASE.into(),
                et - A::KERNBASE,
                A::KERNBASE.into(),
                (AccessFlags::R | AccessFlags::X).into(),
                allocator,
            )
            .ok()?;

        // Map kernel data and the physical RAM we'll make use of.
        page_table
            .insert_range(
                et.into(),
                PHYSTOP - et,
                et.into(),
                (AccessFlags::R | AccessFlags::W).into(),
                allocator,
            )
            .ok()?;

        // Allocate a page for the process's kernel stack.
        // Map it high in memory, followed by an invalid
        // guard page.
        for i in 0..NPROC {
            let pa = allocator.alloc(None)?.into_usize();
            let va: usize = kstack(i);
            page_table
                .insert_range(
                    va.into(),
                    PGSIZE,
                    pa.into(),
                    (AccessFlags::R | AccessFlags::W).into(),
                    allocator,
                )
                .ok()?;
        }

        Some(Self {
            page_table: scopeguard::ScopeGuard::into_inner(page_table),
            _marker: PhantomData,
        })
    }

    /// Initialize register(s) for turning MMU on.
    ///
    /// # Safety
    ///
    /// `self.page_table` must contain base address for a valid page table.
    pub unsafe fn init_register(&self) {
        // SAFETY: `self.page_table` contains valid page table address.
        unsafe {
            A::switch_page_table_and_enable_mmu(self.page_table.as_usize());
        }
    }
}
