use crate::libc;
use crate::{
    kalloc::{kalloc, kfree},
    memlayout::{CLINT, KERNBASE, PHYSTOP, PLIC, TRAMPOLINE, UART0, VIRTIO0},
    printf::{panic, printf},
    riscv::{
        make_satp, pa2pte, pgrounddown, pgroundup, pte2pa, pte_flags, px, sfence_vma, w_satp,
        PagetableT, PdeT, PteT, MAXVA, PGSIZE, PTE_R, PTE_U, PTE_V, PTE_W, PTE_X,
    },
};
use core::ptr;
extern "C" {
    // kernel.ld sets this to end of kernel code.
    #[no_mangle]
    static mut etext: [libc::CChar; 0];

    // trampoline.S
    #[no_mangle]
    static mut trampoline: [libc::CChar; 0];
}

/*
 * the kernel's page table.
 */
pub static mut KERNEL_PAGETABLE: PagetableT = ptr::null_mut();

// trampoline.S
/// create a direct-map page table for the kernel and
/// turn on paging. called early, in supervisor mode.
/// the page allocator is already initialized.
pub unsafe fn kvminit() {
    KERNEL_PAGETABLE = kalloc() as PagetableT;
    ptr::write_bytes(KERNEL_PAGETABLE as *mut libc::CVoid, 0, PGSIZE);

    // uart registers
    kvmmap(UART0, UART0, PGSIZE, (PTE_R | PTE_W) as i32);

    // virtio mmio disk interface
    kvmmap(VIRTIO0, VIRTIO0, PGSIZE, (PTE_R | PTE_W) as i32);

    // CLINT
    kvmmap(CLINT, CLINT, 0x10000, (PTE_R | PTE_W) as i32);

    // PLIC
    kvmmap(PLIC, PLIC, 0x400000, (PTE_R | PTE_W) as i32);

    // map kernel text executable and read-only.
    kvmmap(
        KERNBASE,
        KERNBASE,
        (etext.as_mut_ptr() as usize).wrapping_sub(KERNBASE),
        (PTE_R | PTE_X) as i32,
    );

    // map kernel data and the physical RAM we'll make use of.
    kvmmap(
        etext.as_mut_ptr() as usize,
        etext.as_mut_ptr() as usize,
        (PHYSTOP).wrapping_sub(etext.as_mut_ptr() as usize),
        (PTE_R | PTE_W) as i32,
    );

    // map the trampoline for trap entry/exit to
    // the highest virtual address in the kernel.
    kvmmap(
        TRAMPOLINE,
        trampoline.as_mut_ptr() as usize,
        PGSIZE,
        (PTE_R | PTE_X) as i32,
    );
}

/// Switch h/w page table register to the kernel's page table,
/// and enable paging.
pub unsafe fn kvminithart() {
    w_satp(make_satp(KERNEL_PAGETABLE as usize));
    sfence_vma();
}

/// Return the address of the PTE in page table pagetable
/// that corresponds to virtual address va.  If alloc!=0,
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
unsafe fn walk(mut pagetable: PagetableT, va: usize, alloc: i32) -> *mut PteT {
    if va >= MAXVA {
        panic(b"walk\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
    }
    for level in (1..3).rev() {
        let pte: *mut PteT = &mut *pagetable.add(px(level, va)) as *mut usize;
        if *pte & PTE_V as usize != 0 {
            pagetable = pte2pa(*pte) as PagetableT
        } else {
            if alloc == 0 || {
                pagetable = kalloc() as *mut PdeT;
                pagetable.is_null()
            } {
                return ptr::null_mut();
            }
            ptr::write_bytes(pagetable as *mut libc::CVoid, 0, PGSIZE);
            *pte = pa2pte(pagetable as usize) | PTE_V as usize
        }
    }
    &mut *pagetable.add(px(0, va)) as *mut usize
}

/// Look up a virtual address, return the physical address,
/// or 0 if not mapped.
/// Can only be used to look up user pages.
pub unsafe fn walkaddr(pagetable: PagetableT, va: usize) -> usize {
    if va >= MAXVA {
        return 0;
    }
    let pte: *mut PteT = walk(pagetable, va, 0);
    if pte.is_null() {
        return 0;
    }
    if *pte & PTE_V as usize == 0 {
        return 0;
    }
    if *pte & PTE_U as usize == 0 {
        return 0;
    }
    let pa: usize = pte2pa(*pte);
    pa
}

/// add a mapping to the kernel page table.
/// only used when booting.
/// does not flush TLB or enable paging.
pub unsafe fn kvmmap(va: usize, pa: usize, sz: usize, perm: i32) {
    if mappages(KERNEL_PAGETABLE, va, sz, pa, perm) != 0 {
        panic(b"kvmmap\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
    };
}

/// translate a kernel virtual address to
/// a physical address. only needed for
/// addresses on the stack.
/// assumes va is page aligned.
pub unsafe fn kvmpa(va: usize) -> usize {
    let off: usize = va.wrapping_rem(PGSIZE);
    let pte: *mut PteT = walk(KERNEL_PAGETABLE, va, 0);
    if pte.is_null() {
        panic(b"kvmpa\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
    }
    if *pte & PTE_V as usize == 0 {
        panic(b"kvmpa\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
    }
    let pa: usize = pte2pa(*pte);
    pa.wrapping_add(off)
}

/// Create PTEs for virtual addresses starting at va that refer to
/// physical addresses starting at pa. va and size might not
/// be page-aligned. Returns 0 on success, -1 if walk() couldn't
/// allocate a needed page-table page.
pub unsafe fn mappages(
    pagetable: PagetableT,
    va: usize,
    size: usize,
    mut pa: usize,
    perm: i32,
) -> i32 {
    let mut a = pgrounddown(va);
    let last = pgrounddown(va.wrapping_add(size).wrapping_sub(1usize));
    loop {
        let pte = walk(pagetable, a, 1);
        if pte.is_null() {
            return -1;
        }
        if *pte & PTE_V as usize != 0 {
            panic(b"remap\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
        }
        *pte = pa2pte(pa) | perm as usize | PTE_V as usize;
        if a == last {
            break;
        }
        a = a.wrapping_add(PGSIZE);
        pa = pa.wrapping_add(PGSIZE);
    }
    0
}

/// Remove mappings from a page table. The mappings in
/// the given range must exist. Optionally free the
/// physical memory.
pub unsafe fn uvmunmap(pagetable: PagetableT, va: usize, size: usize, do_free: i32) {
    let mut pa: usize = 0;
    let mut a = pgrounddown(va);
    let last = pgrounddown(va.wrapping_add(size).wrapping_sub(1usize));
    loop {
        let pte = walk(pagetable, a, 0);
        if pte.is_null() {
            panic(b"uvmunmap: walk\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
        }
        if *pte & PTE_V as usize == 0 {
            printf(
                b"va=%p pte=%p\n\x00" as *const u8 as *const libc::CChar as *mut libc::CChar,
                a,
                *pte,
            );
            panic(
                b"uvmunmap: not mapped\x00" as *const u8 as *const libc::CChar as *mut libc::CChar,
            );
        }
        if pte_flags(*pte) == PTE_V as usize {
            panic(
                b"uvmunmap: not a leaf\x00" as *const u8 as *const libc::CChar as *mut libc::CChar,
            );
        }
        if do_free != 0 {
            pa = pte2pa(*pte);
            kfree(pa as *mut libc::CVoid);
        }
        *pte = 0 as PteT;
        if a == last {
            break;
        }
        a = a.wrapping_add(PGSIZE);
        pa = pa.wrapping_add(PGSIZE);
    }
}

/// create an empty user page table.
pub unsafe fn uvmcreate() -> PagetableT {
    let pagetable: PagetableT = kalloc() as PagetableT;
    if pagetable.is_null() {
        panic(
            b"uvmcreate: out of memory\x00" as *const u8 as *const libc::CChar as *mut libc::CChar,
        );
    }
    ptr::write_bytes(pagetable as *mut libc::CVoid, 0, PGSIZE);
    pagetable
}

/// Load the user initcode into address 0 of pagetable,
/// for the very first process.
/// sz must be less than a page.
pub unsafe fn uvminit(pagetable: PagetableT, src: *mut u8, sz: u32) {
    if sz >= PGSIZE as u32 {
        panic(
            b"inituvm: more than a page\x00" as *const u8 as *const libc::CChar as *mut libc::CChar,
        );
    }
    let mem: *mut libc::CChar = kalloc() as *mut libc::CChar;
    ptr::write_bytes(mem as *mut libc::CVoid, 0, PGSIZE);
    mappages(
        pagetable,
        0,
        PGSIZE,
        mem as usize,
        (PTE_W | PTE_R | PTE_X | PTE_U) as i32,
    );
    ptr::copy(
        src as *const libc::CVoid,
        mem as *mut libc::CVoid,
        sz as usize,
    );
}

/// Allocate PTEs and physical memory to grow process from oldsz to
/// newsz, which need not be page aligned.  Returns new size or 0 on error.
pub unsafe fn uvmalloc(pagetable: PagetableT, mut oldsz: usize, newsz: usize) -> usize {
    if newsz < oldsz {
        return oldsz;
    }
    oldsz = pgroundup(oldsz);
    let mut a = oldsz;
    while a < newsz {
        let mem = kalloc() as *mut libc::CChar;
        if mem.is_null() {
            uvmdealloc(pagetable, a, oldsz);
            return 0;
        }
        ptr::write_bytes(mem as *mut libc::CVoid, 0, PGSIZE);
        if mappages(
            pagetable,
            a,
            PGSIZE,
            mem as usize,
            (PTE_W | PTE_X | PTE_R | PTE_U) as i32,
        ) != 0
        {
            kfree(mem as *mut libc::CVoid);
            uvmdealloc(pagetable, a, oldsz);
            return 0;
        }
        a = a.wrapping_add(PGSIZE);
    }
    newsz
}

/// Deallocate user pages to bring the process size from oldsz to
/// newsz.  oldsz and newsz need not be page-aligned, nor does newsz
/// need to be less than oldsz.  oldsz can be larger than the actual
/// process size.  Returns the new process size.
pub unsafe fn uvmdealloc(pagetable: PagetableT, oldsz: usize, newsz: usize) -> usize {
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
unsafe fn freewalk(pagetable: PagetableT) {
    // there are 2^9 = 512 PTEs in a page table.
    for i in 0..512 {
        let pte: PteT = *pagetable.offset(i as isize);
        if pte & PTE_V as usize != 0 && pte & (PTE_R | PTE_W | PTE_X) as usize == 0 {
            // this PTE points to a lower-level page table.
            let child: usize = pte2pa(pte);
            freewalk(child as PagetableT);
            *pagetable.offset(i as isize) = 0
        } else if pte & PTE_V as usize != 0 {
            panic(b"freewalk: leaf\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
        }
    }
    kfree(pagetable as *mut libc::CVoid);
}

/// Free user memory pages,
/// then free page-table pages.
pub unsafe fn uvmfree(pagetable: PagetableT, sz: usize) {
    uvmunmap(pagetable, 0, sz, 1);
    freewalk(pagetable);
}

/// Given a parent process's page table, copy
/// its memory into a child's page table.
/// Copies both the page table and the
/// physical memory.
/// returns 0 on success, -1 on failure.
/// frees any allocated pages on failure.
pub unsafe fn uvmcopy(old: PagetableT, new: PagetableT, sz: usize) -> i32 {
    let current_block: usize;
    let mut i: usize = 0;
    loop {
        if i >= sz {
            current_block = 12349973810996921269;
            break;
        }
        let pte = walk(old, i, 0);
        if pte.is_null() {
            panic(
                b"uvmcopy: pte should exist\x00" as *const u8 as *const libc::CChar
                    as *mut libc::CChar,
            );
        }
        if *pte & PTE_V as usize == 0 {
            panic(
                b"uvmcopy: page not present\x00" as *const u8 as *const libc::CChar
                    as *mut libc::CChar,
            );
        }
        let pa = pte2pa(*pte);
        let flags = pte_flags(*pte) as u32;
        let mem = kalloc() as *mut libc::CChar;
        if mem.is_null() {
            current_block = 9000140654394160520;
            break;
        }
        ptr::copy(
            pa as *mut libc::CChar as *const libc::CVoid,
            mem as *mut libc::CVoid,
            PGSIZE,
        );
        if mappages(new, i, PGSIZE, mem as usize, flags as i32) != 0 {
            kfree(mem as *mut libc::CVoid);
            current_block = 9000140654394160520;
            break;
        } else {
            i = i.wrapping_add(PGSIZE);
        }
    }
    match current_block {
        12349973810996921269 => 0,
        _ => {
            uvmunmap(new, 0, i, 1);
            -1
        }
    }
}

/// mark a PTE invalid for user access.
/// used by exec for the user stack guard page.
pub unsafe fn uvmclear(pagetable: PagetableT, va: usize) {
    let pte: *mut PteT = walk(pagetable, va, 0);
    if pte.is_null() {
        panic(b"uvmclear\x00" as *const u8 as *const libc::CChar as *mut libc::CChar);
    }
    *pte &= !PTE_U as usize;
}

/// Copy from kernel to user.
/// Copy len bytes from src to virtual address dstva in a given page table.
/// Return 0 on success, -1 on error.
pub unsafe fn copyout(
    pagetable: PagetableT,
    mut dstva: usize,
    mut src: *mut libc::CChar,
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
    pagetable: PagetableT,
    mut dst: *mut libc::CChar,
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
    pagetable: PagetableT,
    mut dst: *mut libc::CChar,
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
        let mut p: *mut libc::CChar = pa0.wrapping_add(srcva.wrapping_sub(va0)) as *mut libc::CChar;
        while n > 0 {
            if *p as i32 == '\u{0}' as i32 {
                *dst = '\u{0}' as i32 as libc::CChar;
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
