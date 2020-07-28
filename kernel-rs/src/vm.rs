use crate::libc;
use crate::memlayout::{CLINT, KERNBASE, PHYSTOP, PLIC, TRAMPOLINE, UART0, VIRTIO0};
use crate::riscv::{
    pagetable_t, pte_t, sfence_vma, w_satp, MAXVA, PGSHIFT, PGSIZE, PTE_R, PTE_U, PTE_V, PTE_W,
    PTE_X, PXMASK, SATP_SV39,
};
use crate::types::pde_t;
use core::ptr;
extern "C" {
    // kalloc.c
    #[no_mangle]
    fn kalloc() -> *mut libc::c_void;
    #[no_mangle]
    fn kfree(_: *mut libc::c_void);
    // printf.c
    #[no_mangle]
    fn printf(_: *mut libc::c_char, _: ...);
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    #[no_mangle]
    fn memmove(_: *mut libc::c_void, _: *const libc::c_void, _: u32) -> *mut libc::c_void;
    #[no_mangle]
    fn memset(_: *mut libc::c_void, _: i32, _: u32) -> *mut libc::c_void;
    #[no_mangle]
    static mut etext: [libc::c_char; 0];
    // kernel.ld sets this to end of kernel code.
    #[no_mangle]
    static mut trampoline: [libc::c_char; 0];
}
/*
 * the kernel's page table.
 */
#[no_mangle]
pub static mut kernel_pagetable: pagetable_t = 0 as *const u64 as *mut u64;
/// create a direct-map page table for the kernel and
/// turn on paging. called early, in supervisor mode.
/// the page allocator is already initialized.
#[no_mangle]
pub unsafe extern "C" fn kvminit() {
    kernel_pagetable = kalloc() as pagetable_t;
    memset(kernel_pagetable as *mut libc::c_void, 0, PGSIZE as u32);
    // uart registers
    kvmmap(
        UART0 as u64,
        UART0 as u64,
        PGSIZE as u64,
        (PTE_R | PTE_W) as i32,
    );
    // virtio mmio disk interface
    kvmmap(
        VIRTIO0 as u64,
        VIRTIO0 as u64,
        PGSIZE as u64,
        (PTE_R | PTE_W) as i32,
    );
    // CLINT
    kvmmap(CLINT as u64, CLINT as u64, 0x10000, (PTE_R | PTE_W) as i32);
    // PLIC
    kvmmap(PLIC as u64, PLIC as u64, 0x400000, (PTE_R | PTE_W) as i32);
    // map kernel text executable and read-only.
    kvmmap(
        KERNBASE as u64,
        KERNBASE as u64,
        (etext.as_mut_ptr() as u64).wrapping_sub(KERNBASE as u64),
        (PTE_R | PTE_X) as i32,
    );
    // map kernel data and the physical RAM we'll make use of.
    kvmmap(
        etext.as_mut_ptr() as u64,
        etext.as_mut_ptr() as u64,
        (PHYSTOP as u64).wrapping_sub(etext.as_mut_ptr() as u64),
        (PTE_R | PTE_W) as i32,
    );
    // map the trampoline for trap entry/exit to
    // the highest virtual address in the kernel.
    kvmmap(
        TRAMPOLINE as u64,
        trampoline.as_mut_ptr() as u64,
        PGSIZE as u64,
        (PTE_R | PTE_X) as i32,
    );
}
/// Switch h/w page table register to the kernel's page table,
/// and enable paging.
#[no_mangle]
pub unsafe extern "C" fn kvminithart() {
    w_satp(SATP_SV39 as u64 | kernel_pagetable as u64 >> 12 as i32);
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
unsafe extern "C" fn walk(mut pagetable: pagetable_t, mut va: u64, mut alloc: i32) -> *mut pte_t {
    if va >= MAXVA as u64 {
        panic(b"walk\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    let mut level: i32 = 2;
    while level > 0 {
        let mut pte: *mut pte_t = &mut *pagetable
            .offset((va >> (PGSHIFT + 9 * level) & PXMASK as u64) as isize)
            as *mut u64;
        if *pte & PTE_V as u64 != 0 {
            pagetable = ((*pte >> 10 as i32) << 12 as i32) as pagetable_t
        } else {
            if alloc == 0 || {
                pagetable = kalloc() as *mut pde_t;
                pagetable.is_null()
            } {
                return ptr::null_mut();
            }
            memset(pagetable as *mut libc::c_void, 0, PGSIZE as u32);
            *pte = (pagetable as u64 >> 12 as i32) << 10 as i32 | PTE_V as u64
        }
        level -= 1
    }
    &mut *pagetable.offset((va >> (PGSHIFT + 9 * 0) & PXMASK as u64) as isize) as *mut u64
}
/// Look up a virtual address, return the physical address,
/// or 0 if not mapped.
/// Can only be used to look up user pages.
#[no_mangle]
pub unsafe extern "C" fn walkaddr(mut pagetable: pagetable_t, mut va: u64) -> u64 {
    let mut pte: *mut pte_t = ptr::null_mut();
    let mut pa: u64 = 0;
    if va >= MAXVA as u64 {
        return 0;
    }
    pte = walk(pagetable, va, 0);
    if pte.is_null() {
        return 0;
    }
    if *pte & PTE_V as u64 == 0 as i32 as u64 {
        return 0;
    }
    if *pte & PTE_U as u64 == 0 as i32 as u64 {
        return 0;
    }
    pa = (*pte >> 10 as i32) << 12 as i32;
    pa
}
/// add a mapping to the kernel page table.
/// only used when booting.
/// does not flush TLB or enable paging.
#[no_mangle]
pub unsafe extern "C" fn kvmmap(mut va: u64, mut pa: u64, mut sz: u64, mut perm: i32) {
    if mappages(kernel_pagetable, va, sz, pa, perm) != 0 as i32 {
        panic(b"kvmmap\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    };
}
/// translate a kernel virtual address to
/// a physical address. only needed for
/// addresses on the stack.
/// assumes va is page aligned.
#[no_mangle]
pub unsafe extern "C" fn kvmpa(mut va: u64) -> u64 {
    let mut off: u64 = va.wrapping_rem(PGSIZE as u64);
    let mut pte: *mut pte_t = ptr::null_mut();
    let mut pa: u64 = 0;
    pte = walk(kernel_pagetable, va, 0 as i32);
    if pte.is_null() {
        panic(b"kvmpa\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if *pte & PTE_V as u64 == 0 as i32 as u64 {
        panic(b"kvmpa\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    pa = (*pte >> 10 as i32) << 12 as i32;
    pa.wrapping_add(off)
}
/// Create PTEs for virtual addresses starting at va that refer to
/// physical addresses starting at pa. va and size might not
/// be page-aligned. Returns 0 on success, -1 if walk() couldn't
/// allocate a needed page-table page.
#[no_mangle]
pub unsafe extern "C" fn mappages(
    mut pagetable: pagetable_t,
    mut va: u64,
    mut size: u64,
    mut pa: u64,
    mut perm: i32,
) -> i32 {
    let mut a: u64 = 0;
    let mut last: u64 = 0;
    let mut pte: *mut pte_t = ptr::null_mut();
    a = va & !(PGSIZE - 1 as i32) as u64;
    last = va.wrapping_add(size).wrapping_sub(1 as i32 as u64) & !(PGSIZE - 1 as i32) as u64;
    loop {
        pte = walk(pagetable, a, 1 as i32);
        if pte.is_null() {
            return -(1 as i32);
        }
        if *pte & PTE_V as u64 != 0 {
            panic(b"remap\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        *pte = (pa >> 12 as i32) << 10 as i32 | perm as u64 | PTE_V as u64;
        if a == last {
            break;
        }
        a = (a as u64).wrapping_add(PGSIZE as u64) as u64 as u64;
        pa = (pa as u64).wrapping_add(PGSIZE as u64) as u64 as u64
    }
    0
}
/// Remove mappings from a page table. The mappings in
/// the given range must exist. Optionally free the
/// physical memory.
#[no_mangle]
pub unsafe extern "C" fn uvmunmap(
    mut pagetable: pagetable_t,
    mut va: u64,
    mut size: u64,
    mut do_free: i32,
) {
    let mut a: u64 = 0;
    let mut last: u64 = 0;
    let mut pte: *mut pte_t = ptr::null_mut();
    let mut pa: u64 = 0;
    a = va & !(PGSIZE - 1) as u64;
    last = va.wrapping_add(size).wrapping_sub(1) & !(PGSIZE - 1) as u64;
    loop {
        pte = walk(pagetable, a, 0);
        if pte.is_null() {
            panic(b"uvmunmap: walk\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        if *pte & PTE_V as u64 == 0 as i32 as u64 {
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
        if *pte & 0x3ff as i32 as u64 == PTE_V as u64 {
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
        a = (a as u64).wrapping_add(PGSIZE as u64) as u64 as u64;
        pa = (pa as u64).wrapping_add(PGSIZE as u64) as u64 as u64
    }
}
/// create an empty user page table.
#[no_mangle]
pub unsafe extern "C" fn uvmcreate() -> pagetable_t {
    let mut pagetable: pagetable_t = ptr::null_mut();
    pagetable = kalloc() as pagetable_t;
    if pagetable.is_null() {
        panic(
            b"uvmcreate: out of memory\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    memset(pagetable as *mut libc::c_void, 0, PGSIZE as u32);
    pagetable
}
/// Load the user initcode into address 0 of pagetable,
/// for the very first process.
/// sz must be less than a page.
#[no_mangle]
pub unsafe extern "C" fn uvminit(mut pagetable: pagetable_t, mut src: *mut u8, mut sz: u32) {
    let mut mem: *mut libc::c_char = ptr::null_mut();
    if sz >= PGSIZE as u32 {
        panic(
            b"inituvm: more than a page\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    mem = kalloc() as *mut libc::c_char;
    memset(mem as *mut libc::c_void, 0 as i32, PGSIZE as u32);
    mappages(
        pagetable,
        0,
        PGSIZE as u64,
        mem as u64,
        (PTE_W | PTE_R | PTE_X | PTE_U) as i32,
    );
    memmove(mem as *mut libc::c_void, src as *const libc::c_void, sz);
}
/// Allocate PTEs and physical memory to grow process from oldsz to
/// newsz, which need not be page aligned.  Returns new size or 0 on error.
#[no_mangle]
pub unsafe extern "C" fn uvmalloc(
    mut pagetable: pagetable_t,
    mut oldsz: u64,
    mut newsz: u64,
) -> u64 {
    let mut mem: *mut libc::c_char = ptr::null_mut();
    let mut a: u64 = 0;
    if newsz < oldsz {
        return oldsz;
    }
    oldsz = oldsz
        .wrapping_add(PGSIZE as u64)
        .wrapping_sub(1 as i32 as u64)
        & !(PGSIZE - 1 as i32) as u64;
    a = oldsz;
    while a < newsz {
        mem = kalloc() as *mut libc::c_char;
        if mem.is_null() {
            uvmdealloc(pagetable, a, oldsz);
            return 0 as i32 as u64;
        }
        memset(mem as *mut libc::c_void, 0 as i32, PGSIZE as u32);
        if mappages(
            pagetable,
            a,
            PGSIZE as u64,
            mem as u64,
            (PTE_W | PTE_X | PTE_R | PTE_U) as i32,
        ) != 0 as i32
        {
            kfree(mem as *mut libc::c_void);
            uvmdealloc(pagetable, a, oldsz);
            return 0 as i32 as u64;
        }
        a = (a as u64).wrapping_add(PGSIZE as u64) as u64 as u64
    }
    newsz
}
/// Deallocate user pages to bring the process size from oldsz to
/// newsz.  oldsz and newsz need not be page-aligned, nor does newsz
/// need to be less than oldsz.  oldsz can be larger than the actual
/// process size.  Returns the new process size.
#[no_mangle]
pub unsafe extern "C" fn uvmdealloc(
    mut pagetable: pagetable_t,
    mut oldsz: u64,
    mut newsz: u64,
) -> u64 {
    if newsz >= oldsz {
        return oldsz;
    }
    let mut newup: u64 = newsz
        .wrapping_add(PGSIZE as u64)
        .wrapping_sub(1 as i32 as u64)
        & !(PGSIZE - 1 as i32) as u64;
    if newup
        < oldsz
            .wrapping_add(PGSIZE as u64)
            .wrapping_sub(1 as i32 as u64)
            & !(PGSIZE - 1 as i32) as u64
    {
        uvmunmap(pagetable, newup, oldsz.wrapping_sub(newup), 1 as i32);
    }
    newsz
}
/// Recursively free page-table pages.
/// All leaf mappings must already have been removed.
unsafe extern "C" fn freewalk(mut pagetable: pagetable_t) {
    // there are 2^9 = 512 PTEs in a page table.
    let mut i: i32 = 0;
    while i < 512 {
        let mut pte: pte_t = *pagetable.offset(i as isize);
        if pte & PTE_V as u64 != 0 && pte & (PTE_R | PTE_W | PTE_X) as u64 == 0 as i32 as u64 {
            // this PTE points to a lower-level page table.
            let mut child: u64 = (pte >> 10 as i32) << 12 as i32;
            freewalk(child as pagetable_t);
            *pagetable.offset(i as isize) = 0 as i32 as u64
        } else if pte & PTE_V as u64 != 0 {
            panic(b"freewalk: leaf\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        i += 1
    }
    kfree(pagetable as *mut libc::c_void);
}
/// Free user memory pages,
/// then free page-table pages.
#[no_mangle]
pub unsafe extern "C" fn uvmfree(mut pagetable: pagetable_t, mut sz: u64) {
    uvmunmap(pagetable, 0 as i32 as u64, sz, 1 as i32);
    freewalk(pagetable);
}
/// Given a parent process's page table, copy
/// its memory into a child's page table.
/// Copies both the page table and the
/// physical memory.
/// returns 0 on success, -1 on failure.
/// frees any allocated pages on failure.
#[no_mangle]
pub unsafe extern "C" fn uvmcopy(mut old: pagetable_t, mut new: pagetable_t, mut sz: u64) -> i32 {
    let mut current_block: u64;
    let mut pte: *mut pte_t = ptr::null_mut();
    let mut pa: u64 = 0;
    let mut i: u64 = 0;
    let mut flags: u32 = 0;
    let mut mem: *mut libc::c_char = ptr::null_mut();
    i = 0;
    loop {
        if i >= sz {
            current_block = 12349973810996921269;
            break;
        }
        pte = walk(old, i, 0 as i32);
        if pte.is_null() {
            panic(
                b"uvmcopy: pte should exist\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
            );
        }
        if *pte & PTE_V as u64 == 0 as i32 as u64 {
            panic(
                b"uvmcopy: page not present\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
            );
        }
        pa = (*pte >> 10 as i32) << 12 as i32;
        flags = (*pte & 0x3ff as i32 as u64) as u32;
        mem = kalloc() as *mut libc::c_char;
        if mem.is_null() {
            current_block = 9000140654394160520;
            break;
        }
        memmove(
            mem as *mut libc::c_void,
            pa as *mut libc::c_char as *const libc::c_void,
            PGSIZE as u32,
        );
        if mappages(new, i, PGSIZE as u64, mem as u64, flags as i32) != 0 as i32 {
            kfree(mem as *mut libc::c_void);
            current_block = 9000140654394160520;
            break;
        } else {
            i = (i as u64).wrapping_add(PGSIZE as u64) as u64 as u64
        }
    }
    match current_block {
        12349973810996921269 => 0 as i32,
        _ => {
            uvmunmap(new, 0 as i32 as u64, i, 1 as i32);
            -(1 as i32)
        }
    }
}
/// mark a PTE invalid for user access.
/// used by exec for the user stack guard page.
#[no_mangle]
pub unsafe extern "C" fn uvmclear(mut pagetable: pagetable_t, mut va: u64) {
    let mut pte: *mut pte_t = ptr::null_mut();
    pte = walk(pagetable, va, 0 as i32);
    if pte.is_null() {
        panic(b"uvmclear\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    *pte &= !PTE_U as u64;
}
/// Copy from kernel to user.
/// Copy len bytes from src to virtual address dstva in a given page table.
/// Return 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn copyout(
    mut pagetable: pagetable_t,
    mut dstva: u64,
    mut src: *mut libc::c_char,
    mut len: u64,
) -> i32 {
    let mut n: u64 = 0;
    let mut va0: u64 = 0;
    let mut pa0: u64 = 0;
    while len > 0 as u64 {
        va0 = dstva & !(PGSIZE - 1 as i32) as u64;
        pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 as u64 {
            return -1;
        }
        n = (PGSIZE as u64).wrapping_sub(dstva.wrapping_sub(va0));
        if n > len {
            n = len
        }
        memmove(
            pa0.wrapping_add(dstva.wrapping_sub(va0)) as *mut libc::c_void,
            src as *const libc::c_void,
            n as u32,
        );
        len = (len as u64).wrapping_sub(n) as u64 as u64;
        src = src.offset(n as isize);
        dstva = va0.wrapping_add(PGSIZE as u64)
    }
    0
}
/// Copy from user to kernel.
/// Copy len bytes to dst from virtual address srcva in a given page table.
/// Return 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn copyin(
    mut pagetable: pagetable_t,
    mut dst: *mut libc::c_char,
    mut srcva: u64,
    mut len: u64,
) -> i32 {
    let mut n: u64 = 0;
    let mut va0: u64 = 0;
    let mut pa0: u64 = 0;
    while len > 0 as u64 {
        va0 = srcva & !(PGSIZE - 1) as u64;
        pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 as u64 {
            return -1;
        }
        n = (PGSIZE as u64).wrapping_sub(srcva.wrapping_sub(va0));
        if n > len {
            n = len
        }
        memmove(
            dst as *mut libc::c_void,
            pa0.wrapping_add(srcva.wrapping_sub(va0)) as *mut libc::c_void,
            n as u32,
        );
        len = (len as u64).wrapping_sub(n) as u64 as u64;
        dst = dst.offset(n as isize);
        srcva = va0.wrapping_add(PGSIZE as u64)
    }
    0
}
/// Copy a null-terminated string from user to kernel.
/// Copy bytes to dst from virtual address srcva in a given page table,
/// until a '\0', or max.
/// Return 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn copyinstr(
    mut pagetable: pagetable_t,
    mut dst: *mut libc::c_char,
    mut srcva: u64,
    mut max: u64,
) -> i32 {
    let mut n: u64 = 0;
    let mut va0: u64 = 0;
    let mut pa0: u64 = 0;
    let mut got_null: i32 = 0;
    while got_null == 0 && max > 0 as u64 {
        va0 = srcva & !(PGSIZE - 1) as u64;
        pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 as u64 {
            return -1;
        }
        n = (PGSIZE as u64).wrapping_sub(srcva.wrapping_sub(va0));
        if n > max {
            n = max
        }
        let mut p: *mut libc::c_char =
            pa0.wrapping_add(srcva.wrapping_sub(va0)) as *mut libc::c_char;
        while n > 0 as u64 {
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
        srcva = va0.wrapping_add(PGSIZE as u64)
    }
    if got_null != 0 {
        0
    } else {
        -1
    }
}
