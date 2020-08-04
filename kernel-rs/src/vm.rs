use crate::libc;
use crate::{
    kalloc::{kalloc, kfree},
    memlayout::{CLINT, KERNBASE, PHYSTOP, PLIC, TRAMPOLINE, UART0, VIRTIO0},
    printf::{panic, printf},
    riscv::{
        make_satp, pagetable_t, pde_t, pte_t, px, sfence_vma, w_satp, MAXVA, PGSIZE, PTE_R, PTE_U,
        PTE_V, PTE_W, PTE_X,
    },
};
use core::ptr;
extern "C" {
    // kernel.ld sets this to end of kernel code.
    #[no_mangle]
    static mut etext: [libc::c_char; 0];

    // trampoline.S
    #[no_mangle]
    static mut trampoline: [libc::c_char; 0];
}

/*
 * the kernel's page table.
 */
pub static mut kernel_pagetable: pagetable_t = 0 as *const usize as *mut usize;

// trampoline.S
/// create a direct-map page table for the kernel and
/// turn on paging. called early, in supervisor mode.
/// the page allocator is already initialized.
pub unsafe fn kvminit() {
    kernel_pagetable = kalloc() as pagetable_t;
    ptr::write_bytes(kernel_pagetable as *mut libc::c_void, 0, PGSIZE as usize);

    // uart registers
    kvmmap(
        UART0 as usize,
        UART0 as usize,
        PGSIZE as usize,
        (PTE_R | PTE_W) as i32,
    );

    // virtio mmio disk interface
    kvmmap(
        VIRTIO0 as usize,
        VIRTIO0 as usize,
        PGSIZE as usize,
        (PTE_R | PTE_W) as i32,
    );

    // CLINT
    kvmmap(
        CLINT as usize,
        CLINT as usize,
        0x10000,
        (PTE_R | PTE_W) as i32,
    );

    // PLIC
    kvmmap(
        PLIC as usize,
        PLIC as usize,
        0x400000,
        (PTE_R | PTE_W) as i32,
    );

    // map kernel text executable and read-only.
    kvmmap(
        KERNBASE as usize,
        KERNBASE as usize,
        (etext.as_mut_ptr() as usize).wrapping_sub(KERNBASE as usize),
        (PTE_R | PTE_X) as i32,
    );

    // map kernel data and the physical RAM we'll make use of.
    kvmmap(
        etext.as_mut_ptr() as usize,
        etext.as_mut_ptr() as usize,
        (PHYSTOP as usize).wrapping_sub(etext.as_mut_ptr() as usize),
        (PTE_R | PTE_W) as i32,
    );

    // map the trampoline for trap entry/exit to
    // the highest virtual address in the kernel.
    kvmmap(
        TRAMPOLINE as usize,
        trampoline.as_mut_ptr() as usize,
        PGSIZE as usize,
        (PTE_R | PTE_X) as i32,
    );
}

/// Switch h/w page table register to the kernel's page table,
/// and enable paging.
pub unsafe fn kvminithart() {
    w_satp(make_satp(kernel_pagetable as usize));
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
unsafe fn walk(mut pagetable: pagetable_t, mut va: usize, mut alloc: i32) -> *mut pte_t {
    if va >= MAXVA as usize {
        panic(b"walk\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    for level in (1..3).rev() {
        let mut pte: *mut pte_t = &mut *pagetable
            .offset((va >> (PGSHIFT + 9 * level) & PXMASK as usize) as isize)
            as *mut usize;
        if *pte & PTE_V as usize != 0 {
            pagetable = ((*pte >> 10 as i32) << 12 as i32) as pagetable_t
        } else {
            if alloc == 0 || {
                pagetable = kalloc() as *mut pde_t;
                pagetable.is_null()
            } {
                return ptr::null_mut();
            }
            ptr::write_bytes(pagetable as *mut libc::c_void, 0, PGSIZE as usize);
            *pte = (pagetable as usize >> 12 as i32) << 10 as i32 | PTE_V as usize
        }
    }
    &mut *pagetable.add(px(0, va) as usize)

/// Look up a virtual address, return the physical address,
/// or 0 if not mapped.
/// Can only be used to look up user pages.
pub unsafe fn walkaddr(mut pagetable: pagetable_t, mut va: usize) -> usize {
    let mut pte: *mut pte_t = ptr::null_mut();
    let mut pa: usize = 0;
    if va >= MAXVA as usize {
        return 0;
    }
    pte = walk(pagetable, va, 0);
    if pte.is_null() {
        return 0;
    }
    if *pte & PTE_V as usize == 0 as i32 as usize {
        return 0;
    }
    if *pte & PTE_U as usize == 0 as i32 as usize {
        return 0;
    }
    pa = (*pte >> 10 as i32) << 12 as i32;
    pa
}

/// add a mapping to the kernel page table.
/// only used when booting.
/// does not flush TLB or enable paging.
pub unsafe fn kvmmap(mut va: usize, mut pa: usize, mut sz: usize, mut perm: i32) {
    if mappages(kernel_pagetable, va, sz, pa, perm) != 0 as i32 {
        panic(b"kvmmap\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    };
}

/// translate a kernel virtual address to
/// a physical address. only needed for
/// addresses on the stack.
/// assumes va is page aligned.
pub unsafe fn kvmpa(mut va: usize) -> usize {
    let mut off: usize = va.wrapping_rem(PGSIZE as usize);
    let mut pte: *mut pte_t = ptr::null_mut();
    let mut pa: usize = 0;
    pte = walk(kernel_pagetable, va, 0 as i32);
    if pte.is_null() {
        panic(b"kvmpa\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if *pte & PTE_V as usize == 0 as i32 as usize {
        panic(b"kvmpa\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    pa = (*pte >> 10 as i32) << 12 as i32;
    pa.wrapping_add(off)
}

/// Create PTEs for virtual addresses starting at va that refer to
/// physical addresses starting at pa. va and size might not
/// be page-aligned. Returns 0 on success, -1 if walk() couldn't
/// allocate a needed page-table page.
pub unsafe fn mappages(
    mut pagetable: pagetable_t,
    mut va: usize,
    mut size: usize,
    mut pa: usize,
    mut perm: i32,
) -> i32 {
    let mut a = va & !(PGSIZE - 1 as i32) as usize;
    let last =
        va.wrapping_add(size).wrapping_sub(1 as i32 as usize) & !(PGSIZE - 1 as i32) as usize;
    loop {
        let pte = walk(pagetable, a, 1 as i32);
        if pte.is_null() {
            return -(1 as i32);
        }
        if *pte & PTE_V as usize != 0 {
            panic(b"remap\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        *pte = (pa >> 12 as i32) << 10 as i32 | perm as usize | PTE_V as usize;
        if a == last {
            break;
        }
        a = (a as usize).wrapping_add(PGSIZE as usize) as usize as usize;
        pa = (pa as usize).wrapping_add(PGSIZE as usize) as usize as usize
    }
    0
}

/// Remove mappings from a page table. The mappings in
/// the given range must exist. Optionally free the
/// physical memory.
pub unsafe fn uvmunmap(
    mut pagetable: pagetable_t,
    mut va: usize,
    mut size: usize,
    mut do_free: i32,
) {
    let mut pa: usize = 0;
    let mut a = va & !(PGSIZE - 1) as usize;
    let last = va.wrapping_add(size).wrapping_sub(1) & !(PGSIZE - 1) as usize;
    loop {
        let pte = walk(pagetable, a, 0);
        if pte.is_null() {
            panic(b"uvmunmap: walk\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        if *pte & PTE_V as usize == 0 as i32 as usize {
            printf(
                b"va=%p pte=%p\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
                a,
                *pte,
            );
            panic(
                b"uvmunmap: not mapped\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
            );
        }
        if *pte & 0x3ff as i32 as usize == PTE_V as usize {
            panic(
                b"uvmunmap: not a leaf\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
            );
        }
        if do_free != 0 {
            pa = (*pte >> 10 as i32) << 12 as i32;
            kfree(pa as *mut libc::c_void);
        }
        *pte = 0 as i32 as pte_t;
        if a == last {
            break;
        }
        a = (a as usize).wrapping_add(PGSIZE as usize) as usize as usize;
        pa = (pa as usize).wrapping_add(PGSIZE as usize) as usize as usize
    }
}

/// create an empty user page table.
pub unsafe fn uvmcreate() -> pagetable_t {
    let mut pagetable: pagetable_t = ptr::null_mut();
    pagetable = kalloc() as pagetable_t;
    if pagetable.is_null() {
        panic(
            b"uvmcreate: out of memory\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    ptr::write_bytes(pagetable as *mut libc::c_void, 0, PGSIZE as usize);
    pagetable
}

/// Load the user initcode into address 0 of pagetable,
/// for the very first process.
/// sz must be less than a page.
pub unsafe fn uvminit(mut pagetable: pagetable_t, mut src: *mut u8, mut sz: u32) {
    let mut mem: *mut libc::c_char = ptr::null_mut();
    if sz >= PGSIZE as u32 {
        panic(
            b"inituvm: more than a page\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    mem = kalloc() as *mut libc::c_char;
    ptr::write_bytes(mem as *mut libc::c_void, 0, PGSIZE as usize);
    mappages(
        pagetable,
        0,
        PGSIZE as usize,
        mem as usize,
        (PTE_W | PTE_R | PTE_X | PTE_U) as i32,
    );
    ptr::copy(
        src as *const libc::c_void,
        mem as *mut libc::c_void,
        sz as usize,
    );
}

/// Allocate PTEs and physical memory to grow process from oldsz to
/// newsz, which need not be page aligned.  Returns new size or 0 on error.
pub unsafe fn uvmalloc(mut pagetable: pagetable_t, mut oldsz: usize, mut newsz: usize) -> usize {
    if newsz < oldsz {
        return oldsz;
    }
    oldsz = oldsz
        .wrapping_add(PGSIZE as usize)
        .wrapping_sub(1 as i32 as usize)
        & !(PGSIZE - 1 as i32) as usize;
    let mut a = oldsz;
    while a < newsz {
        let mem = kalloc() as *mut libc::c_char;
        if mem.is_null() {
            uvmdealloc(pagetable, a, oldsz);
            return 0 as i32 as usize;
        }
        ptr::write_bytes(mem as *mut libc::c_void, 0, PGSIZE as usize);
        if mappages(
            pagetable,
            a,
            PGSIZE as usize,
            mem as usize,
            (PTE_W | PTE_X | PTE_R | PTE_U) as i32,
        ) != 0 as i32
        {
            kfree(mem as *mut libc::c_void);
            uvmdealloc(pagetable, a, oldsz);
            return 0 as i32 as usize;
        }
        a = (a as usize).wrapping_add(PGSIZE as usize) as usize as usize
    }
    newsz
}

/// Deallocate user pages to bring the process size from oldsz to
/// newsz.  oldsz and newsz need not be page-aligned, nor does newsz
/// need to be less than oldsz.  oldsz can be larger than the actual
/// process size.  Returns the new process size.
pub unsafe fn uvmdealloc(mut pagetable: pagetable_t, mut oldsz: usize, mut newsz: usize) -> usize {
    if newsz >= oldsz {
        return oldsz;
    }
    let mut newup: usize = newsz
        .wrapping_add(PGSIZE as usize)
        .wrapping_sub(1 as i32 as usize)
        & !(PGSIZE - 1 as i32) as usize;
    if newup
        < oldsz
            .wrapping_add(PGSIZE as usize)
            .wrapping_sub(1 as i32 as usize)
            & !(PGSIZE - 1 as i32) as usize
    {
        uvmunmap(pagetable, newup, oldsz.wrapping_sub(newup), 1 as i32);
    }
    newsz
}

/// Recursively free page-table pages.
/// All leaf mappings must already have been removed.
unsafe fn freewalk(mut pagetable: pagetable_t) {
    // there are 2^9 = 512 PTEs in a page table.
    for i in 0..512 {
        let mut pte: pte_t = *pagetable.offset(i as isize);
        if pte & PTE_V as usize != 0 && pte & (PTE_R | PTE_W | PTE_X) as usize == 0 as i32 as usize
        {
            // this PTE points to a lower-level page table.
            let mut child: usize = (pte >> 10 as i32) << 12 as i32;
            freewalk(child as pagetable_t);
            *pagetable.offset(i as isize) = 0 as i32 as usize
        } else if pte & PTE_V as usize != 0 {
            panic(b"freewalk: leaf\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
    }
    kfree(pagetable as *mut libc::c_void);
}

/// Free user memory pages,
/// then free page-table pages.
pub unsafe fn uvmfree(mut pagetable: pagetable_t, mut sz: usize) {
    uvmunmap(pagetable, 0 as i32 as usize, sz, 1 as i32);
    freewalk(pagetable);
}

/// Given a parent process's page table, copy
/// its memory into a child's page table.
/// Copies both the page table and the
/// physical memory.
/// returns 0 on success, -1 on failure.
/// frees any allocated pages on failure.
pub unsafe fn uvmcopy(mut old: pagetable_t, mut new: pagetable_t, mut sz: usize) -> i32 {
    let mut current_block: usize;
    let mut i: usize = 0;
    loop {
        if i >= sz {
            current_block = 12349973810996921269;
            break;
        }
        let pte = walk(old, i, 0 as i32);
        if pte.is_null() {
            panic(
                b"uvmcopy: pte should exist\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
            );
        }
        if *pte & PTE_V as usize == 0 as i32 as usize {
            panic(
                b"uvmcopy: page not present\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
            );
        }
        let pa = (*pte >> 10 as i32) << 12 as i32;
        let flags = (*pte & 0x3ff as i32 as usize) as u32;
        let mem = kalloc() as *mut libc::c_char;
        if mem.is_null() {
            current_block = 9000140654394160520;
            break;
        }
        ptr::copy(
            pa as *mut libc::c_char as *const libc::c_void,
            mem as *mut libc::c_void,
            PGSIZE as usize,
        );
        if mappages(new, i, PGSIZE as usize, mem as usize, flags as i32) != 0 as i32 {
            kfree(mem as *mut libc::c_void);
            current_block = 9000140654394160520;
            break;
        } else {
            i = (i as usize).wrapping_add(PGSIZE as usize) as usize as usize
        }
    }
    match current_block {
        12349973810996921269 => 0 as i32,
        _ => {
            uvmunmap(new, 0 as i32 as usize, i, 1 as i32);
            -(1 as i32)
        }
    }
}

/// mark a PTE invalid for user access.
/// used by exec for the user stack guard page.
pub unsafe fn uvmclear(mut pagetable: pagetable_t, mut va: usize) {
    let mut pte: *mut pte_t = ptr::null_mut();
    pte = walk(pagetable, va, 0 as i32);
    if pte.is_null() {
        panic(b"uvmclear\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    *pte &= !PTE_U as usize;
}

/// Copy from kernel to user.
/// Copy len bytes from src to virtual address dstva in a given page table.
/// Return 0 on success, -1 on error.
pub unsafe fn copyout(
    mut pagetable: pagetable_t,
    mut dstva: usize,
    mut src: *mut libc::c_char,
    mut len: usize,
) -> i32 {
    while len > 0 as usize {
        let mut va0 = dstva & !(PGSIZE - 1 as i32) as usize;
        let pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 as usize {
            return -1;
        }
        let mut n = (PGSIZE as usize).wrapping_sub(dstva.wrapping_sub(va0));
        if n > len {
            n = len
        }
        ptr::copy(
            src as *const libc::c_void,
            pa0.wrapping_add(dstva.wrapping_sub(va0)) as *mut libc::c_void,
            n as usize,
        );
        len = (len as usize).wrapping_sub(n) as usize as usize;
        src = src.offset(n as isize);
        dstva = va0.wrapping_add(PGSIZE as usize)
    }
    0
}

/// Copy from user to kernel.
/// Copy len bytes to dst from virtual address srcva in a given page table.
/// Return 0 on success, -1 on error.
pub unsafe fn copyin(
    mut pagetable: pagetable_t,
    mut dst: *mut libc::c_char,
    mut srcva: usize,
    mut len: usize,
) -> i32 {
    while len > 0 as usize {
        let mut va0 = srcva & !(PGSIZE - 1) as usize;
        let pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 as usize {
            return -1;
        }
        let mut n = (PGSIZE as usize).wrapping_sub(srcva.wrapping_sub(va0));
        if n > len {
            n = len
        }
        ptr::copy(
            pa0.wrapping_add(srcva.wrapping_sub(va0)) as *mut libc::c_void,
            dst as *mut libc::c_void,
            n as usize,
        );
        len = (len as usize).wrapping_sub(n) as usize as usize;
        dst = dst.offset(n as isize);
        srcva = va0.wrapping_add(PGSIZE as usize)
    }
    0
}

/// Copy a null-terminated string from user to kernel.
/// Copy bytes to dst from virtual address srcva in a given page table,
/// until a '\0', or max.
/// Return 0 on success, -1 on error.
pub unsafe fn copyinstr(
    mut pagetable: pagetable_t,
    mut dst: *mut libc::c_char,
    mut srcva: usize,
    mut max: usize,
) -> i32 {
    let mut got_null: i32 = 0;
    while got_null == 0 && max > 0 as usize {
        let mut va0 = srcva & !(PGSIZE - 1) as usize;
        let pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 as usize {
            return -1;
        }
        let mut n = (PGSIZE as usize).wrapping_sub(srcva.wrapping_sub(va0));
        if n > max {
            n = max
        }
        let mut p: *mut libc::c_char =
            pa0.wrapping_add(srcva.wrapping_sub(va0)) as *mut libc::c_char;
        while n > 0 as usize {
            if *p as i32 == '\u{0}' as i32 {
                *dst = '\u{0}' as i32 as libc::c_char;
                got_null = 1 as i32;
                break;
            } else {
                *dst = *p;
                n = n.wrapping_sub(1);
                max = max.wrapping_sub(1);
                p = p.offset(1);
                dst = dst.offset(1)
            }
        }
        srcva = va0.wrapping_add(PGSIZE as usize)
    }
    if got_null != 0 {
        0
    } else {
        -1
    }
}
