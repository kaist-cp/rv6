use crate::libc;
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
    fn memmove(_: *mut libc::c_void, _: *const libc::c_void, _: uint)
     -> *mut libc::c_void;
    #[no_mangle]
    fn memset(_: *mut libc::c_void, _: libc::c_int, _: uint)
     -> *mut libc::c_void;
    #[no_mangle]
    static mut etext: [libc::c_char; 0];
    // kernel.ld sets this to end of kernel code.
    #[no_mangle]
    static mut trampoline: [libc::c_char; 0];
}
pub type uint = libc::c_uint;
pub type uchar = libc::c_uchar;
pub type uint64 = libc::c_ulong;
pub type pde_t = uint64;
pub type pte_t = uint64;
pub type pagetable_t = *mut uint64;
// Physical memory layout
// qemu -machine virt is set up like this,
// based on qemu's hw/riscv/virt.c:
//
// 00001000 -- boot ROM, provided by qemu
// 02000000 -- CLINT
// 0C000000 -- PLIC
// 10000000 -- uart0 
// 10001000 -- virtio disk 
// 80000000 -- boot ROM jumps here in machine mode
//             -kernel loads the kernel here
// unused RAM after 80000000.
// the kernel uses physical memory thus:
// 80000000 -- entry.S, then kernel text and data
// end -- start of kernel page allocation area
// PHYSTOP -- end RAM used by the kernel
// qemu puts UART registers here in physical memory.
pub const UART0: libc::c_long = 0x10000000 as libc::c_long;
// virtio mmio interface
pub const VIRTIO0: libc::c_int = 0x10001000 as libc::c_int;
// local interrupt controller, which contains the timer.
pub const CLINT: libc::c_long = 0x2000000 as libc::c_long;
// cycles since boot.
// qemu puts programmable interrupt controller here.
pub const PLIC: libc::c_long = 0xc000000 as libc::c_long;
// the kernel expects there to be RAM
// for use by the kernel and user pages
// from physical address 0x80000000 to PHYSTOP.
pub const KERNBASE: libc::c_long = 0x80000000 as libc::c_long;
pub const PHYSTOP: libc::c_long =
    KERNBASE +
        (128 as libc::c_int * 1024 as libc::c_int * 1024 as libc::c_int) as
            libc::c_long;
// map the trampoline page to the highest address,
// in both user and kernel space.
pub const TRAMPOLINE: libc::c_long = MAXVA - PGSIZE as libc::c_long;
// use riscv's sv39 page table scheme.
pub const SATP_SV39: libc::c_long = (8 as libc::c_long) << 60 as libc::c_int;
// supervisor address translation and protection;
// holds the address of the page table.
#[inline]
unsafe extern "C" fn w_satp(mut x: uint64) {
    llvm_asm!("csrw satp, $0" : : "r" (x) : : "volatile");
}
// flush the TLB.
#[inline]
unsafe extern "C" fn sfence_vma() {
    // the zero, zero means flush all TLB entries.
    llvm_asm!("sfence.vma zero, zero" : : : : "volatile");
}
pub const PGSIZE: libc::c_int = 4096 as libc::c_int;
// bytes per page
pub const PGSHIFT: libc::c_int = 12 as libc::c_int;
// bits of offset within a page
pub const PTE_V: libc::c_long = (1 as libc::c_long) << 0 as libc::c_int;
// valid
pub const PTE_R: libc::c_long = (1 as libc::c_long) << 1 as libc::c_int;
pub const PTE_W: libc::c_long = (1 as libc::c_long) << 2 as libc::c_int;
pub const PTE_X: libc::c_long = (1 as libc::c_long) << 3 as libc::c_int;
pub const PTE_U: libc::c_long = (1 as libc::c_long) << 4 as libc::c_int;
// 1 -> user can access
// shift a physical address to the right place for a PTE.
// extract the three 9-bit page table indices from a virtual address.
pub const PXMASK: libc::c_int = 0x1ff as libc::c_int;
// 9 bits
// one beyond the highest possible virtual address.
// MAXVA is actually one bit less than the max allowed by
// Sv39, to avoid having to sign-extend virtual addresses
// that have the high bit set.
pub const MAXVA: libc::c_long =
    (1 as libc::c_long) <<
        9 as libc::c_int + 9 as libc::c_int + 9 as libc::c_int +
            12 as libc::c_int - 1 as libc::c_int;
/*
 * the kernel's page table.
 */
#[no_mangle]
pub static mut kernel_pagetable: pagetable_t =
    0 as *const uint64 as *mut uint64;
// vm.c
// trampoline.S
/*
 * create a direct-map page table for the kernel and
 * turn on paging. called early, in supervisor mode.
 * the page allocator is already initialized.
 */
#[no_mangle]
pub unsafe extern "C" fn kvminit() {
    kernel_pagetable = kalloc() as pagetable_t;
    memset(kernel_pagetable as *mut libc::c_void, 0 as libc::c_int,
           PGSIZE as uint);
    // uart registers
    kvmmap(UART0 as uint64, UART0 as uint64, PGSIZE as uint64,
           (PTE_R | PTE_W) as libc::c_int);
    // virtio mmio disk interface
    kvmmap(VIRTIO0 as uint64, VIRTIO0 as uint64, PGSIZE as uint64,
           (PTE_R | PTE_W) as libc::c_int);
    // CLINT
    kvmmap(CLINT as uint64, CLINT as uint64, 0x10000 as libc::c_int as uint64,
           (PTE_R | PTE_W) as libc::c_int);
    // PLIC
    kvmmap(PLIC as uint64, PLIC as uint64, 0x400000 as libc::c_int as uint64,
           (PTE_R | PTE_W) as libc::c_int);
    // map kernel text executable and read-only.
    kvmmap(KERNBASE as uint64, KERNBASE as uint64,
           (etext.as_mut_ptr() as
                uint64).wrapping_sub(KERNBASE as libc::c_ulong),
           (PTE_R | PTE_X) as libc::c_int);
    // map kernel data and the physical RAM we'll make use of.
    kvmmap(etext.as_mut_ptr() as uint64, etext.as_mut_ptr() as uint64,
           (PHYSTOP as
                libc::c_ulong).wrapping_sub(etext.as_mut_ptr() as uint64),
           (PTE_R | PTE_W) as libc::c_int);
    // map the trampoline for trap entry/exit to
  // the highest virtual address in the kernel.
    kvmmap(TRAMPOLINE as uint64, trampoline.as_mut_ptr() as uint64,
           PGSIZE as uint64, (PTE_R | PTE_X) as libc::c_int);
}
// Switch h/w page table register to the kernel's page table,
// and enable paging.
#[no_mangle]
pub unsafe extern "C" fn kvminithart() {
    w_satp(SATP_SV39 as libc::c_ulong |
               kernel_pagetable as uint64 >> 12 as libc::c_int);
    sfence_vma();
}
// Return the address of the PTE in page table pagetable
// that corresponds to virtual address va.  If alloc!=0,
// create any required page-table pages.
//
// The risc-v Sv39 scheme has three levels of page-table
// pages. A page-table page contains 512 64-bit PTEs.
// A 64-bit virtual address is split into five fields:
//   39..63 -- must be zero.
//   30..38 -- 9 bits of level-2 index.
//   21..39 -- 9 bits of level-1 index.
//   12..20 -- 9 bits of level-0 index.
//    0..12 -- 12 bits of byte offset within the page.
unsafe extern "C" fn walk(mut pagetable: pagetable_t, mut va: uint64,
                          mut alloc: libc::c_int) -> *mut pte_t {
    if va >= MAXVA as libc::c_ulong {
        panic(b"walk\x00" as *const u8 as *const libc::c_char as
                  *mut libc::c_char);
    }
    let mut level: libc::c_int = 2 as libc::c_int;
    while level > 0 as libc::c_int {
        let mut pte: *mut pte_t =
            &mut *pagetable.offset((va >> PGSHIFT + 9 as libc::c_int * level &
                                        PXMASK as libc::c_ulong) as isize) as
                *mut uint64;
        if *pte & PTE_V as libc::c_ulong != 0 {
            pagetable =
                ((*pte >> 10 as libc::c_int) << 12 as libc::c_int) as
                    pagetable_t
        } else {
            if alloc == 0 ||
                   { pagetable = kalloc() as *mut pde_t; pagetable.is_null() }
               {
                return 0 as *mut pte_t
            }
            memset(pagetable as *mut libc::c_void, 0 as libc::c_int,
                   PGSIZE as uint);
            *pte =
                (pagetable as uint64 >> 12 as libc::c_int) <<
                    10 as libc::c_int | PTE_V as libc::c_ulong
        }
        level -= 1
    }
    return &mut *pagetable.offset((va >>
                                       PGSHIFT +
                                           9 as libc::c_int * 0 as libc::c_int
                                       & PXMASK as libc::c_ulong) as isize) as
               *mut uint64;
}
// Look up a virtual address, return the physical address,
// or 0 if not mapped.
// Can only be used to look up user pages.
#[no_mangle]
pub unsafe extern "C" fn walkaddr(mut pagetable: pagetable_t, mut va: uint64)
 -> uint64 {
    let mut pte: *mut pte_t = 0 as *mut pte_t;
    let mut pa: uint64 = 0;
    if va >= MAXVA as libc::c_ulong { return 0 as libc::c_int as uint64 }
    pte = walk(pagetable, va, 0 as libc::c_int);
    if pte.is_null() { return 0 as libc::c_int as uint64 }
    if *pte & PTE_V as libc::c_ulong == 0 as libc::c_int as libc::c_ulong {
        return 0 as libc::c_int as uint64
    }
    if *pte & PTE_U as libc::c_ulong == 0 as libc::c_int as libc::c_ulong {
        return 0 as libc::c_int as uint64
    }
    pa = (*pte >> 10 as libc::c_int) << 12 as libc::c_int;
    return pa;
}
// add a mapping to the kernel page table.
// only used when booting.
// does not flush TLB or enable paging.
#[no_mangle]
pub unsafe extern "C" fn kvmmap(mut va: uint64, mut pa: uint64,
                                mut sz: uint64, mut perm: libc::c_int) {
    if mappages(kernel_pagetable, va, sz, pa, perm) != 0 as libc::c_int {
        panic(b"kvmmap\x00" as *const u8 as *const libc::c_char as
                  *mut libc::c_char);
    };
}
// translate a kernel virtual address to
// a physical address. only needed for
// addresses on the stack.
// assumes va is page aligned.
#[no_mangle]
pub unsafe extern "C" fn kvmpa(mut va: uint64) -> uint64 {
    let mut off: uint64 = va.wrapping_rem(PGSIZE as libc::c_ulong);
    let mut pte: *mut pte_t = 0 as *mut pte_t;
    let mut pa: uint64 = 0;
    pte = walk(kernel_pagetable, va, 0 as libc::c_int);
    if pte.is_null() {
        panic(b"kvmpa\x00" as *const u8 as *const libc::c_char as
                  *mut libc::c_char);
    }
    if *pte & PTE_V as libc::c_ulong == 0 as libc::c_int as libc::c_ulong {
        panic(b"kvmpa\x00" as *const u8 as *const libc::c_char as
                  *mut libc::c_char);
    }
    pa = (*pte >> 10 as libc::c_int) << 12 as libc::c_int;
    return pa.wrapping_add(off);
}
// Create PTEs for virtual addresses starting at va that refer to
// physical addresses starting at pa. va and size might not
// be page-aligned. Returns 0 on success, -1 if walk() couldn't
// allocate a needed page-table page.
#[no_mangle]
pub unsafe extern "C" fn mappages(mut pagetable: pagetable_t, mut va: uint64,
                                  mut size: uint64, mut pa: uint64,
                                  mut perm: libc::c_int) -> libc::c_int {
    let mut a: uint64 = 0;
    let mut last: uint64 = 0;
    let mut pte: *mut pte_t = 0 as *mut pte_t;
    a = va & !(PGSIZE - 1 as libc::c_int) as libc::c_ulong;
    last =
        va.wrapping_add(size).wrapping_sub(1 as libc::c_int as libc::c_ulong)
            & !(PGSIZE - 1 as libc::c_int) as libc::c_ulong;
    loop  {
        pte = walk(pagetable, a, 1 as libc::c_int);
        if pte.is_null() { return -(1 as libc::c_int) }
        if *pte & PTE_V as libc::c_ulong != 0 {
            panic(b"remap\x00" as *const u8 as *const libc::c_char as
                      *mut libc::c_char);
        }
        *pte =
            (pa >> 12 as libc::c_int) << 10 as libc::c_int |
                perm as libc::c_ulong | PTE_V as libc::c_ulong;
        if a == last { break ; }
        a =
            (a as libc::c_ulong).wrapping_add(PGSIZE as libc::c_ulong) as
                uint64 as uint64;
        pa =
            (pa as libc::c_ulong).wrapping_add(PGSIZE as libc::c_ulong) as
                uint64 as uint64
    }
    return 0 as libc::c_int;
}
// Remove mappings from a page table. The mappings in
// the given range must exist. Optionally free the
// physical memory.
#[no_mangle]
pub unsafe extern "C" fn uvmunmap(mut pagetable: pagetable_t, mut va: uint64,
                                  mut size: uint64,
                                  mut do_free: libc::c_int) {
    let mut a: uint64 = 0;
    let mut last: uint64 = 0;
    let mut pte: *mut pte_t = 0 as *mut pte_t;
    let mut pa: uint64 = 0;
    a = va & !(PGSIZE - 1 as libc::c_int) as libc::c_ulong;
    last =
        va.wrapping_add(size).wrapping_sub(1 as libc::c_int as libc::c_ulong)
            & !(PGSIZE - 1 as libc::c_int) as libc::c_ulong;
    loop  {
        pte = walk(pagetable, a, 0 as libc::c_int);
        if pte.is_null() {
            panic(b"uvmunmap: walk\x00" as *const u8 as *const libc::c_char as
                      *mut libc::c_char);
        }
        if *pte & PTE_V as libc::c_ulong == 0 as libc::c_int as libc::c_ulong
           {
            printf(b"va=%p pte=%p\n\x00" as *const u8 as *const libc::c_char
                       as *mut libc::c_char, a, *pte);
            panic(b"uvmunmap: not mapped\x00" as *const u8 as
                      *const libc::c_char as *mut libc::c_char);
        }
        if *pte & 0x3ff as libc::c_int as libc::c_ulong ==
               PTE_V as libc::c_ulong {
            panic(b"uvmunmap: not a leaf\x00" as *const u8 as
                      *const libc::c_char as *mut libc::c_char);
        }
        if do_free != 0 {
            pa = (*pte >> 10 as libc::c_int) << 12 as libc::c_int;
            kfree(pa as *mut libc::c_void);
        }
        *pte = 0 as libc::c_int as pte_t;
        if a == last { break ; }
        a =
            (a as libc::c_ulong).wrapping_add(PGSIZE as libc::c_ulong) as
                uint64 as uint64;
        pa =
            (pa as libc::c_ulong).wrapping_add(PGSIZE as libc::c_ulong) as
                uint64 as uint64
    };
}
// create an empty user page table.
#[no_mangle]
pub unsafe extern "C" fn uvmcreate() -> pagetable_t {
    let mut pagetable: pagetable_t = 0 as *mut uint64;
    pagetable = kalloc() as pagetable_t;
    if pagetable.is_null() {
        panic(b"uvmcreate: out of memory\x00" as *const u8 as
                  *const libc::c_char as *mut libc::c_char);
    }
    memset(pagetable as *mut libc::c_void, 0 as libc::c_int, PGSIZE as uint);
    return pagetable;
}
// Load the user initcode into address 0 of pagetable,
// for the very first process.
// sz must be less than a page.
#[no_mangle]
pub unsafe extern "C" fn uvminit(mut pagetable: pagetable_t,
                                 mut src: *mut uchar, mut sz: uint) {
    let mut mem: *mut libc::c_char = 0 as *mut libc::c_char;
    if sz >= PGSIZE as libc::c_uint {
        panic(b"inituvm: more than a page\x00" as *const u8 as
                  *const libc::c_char as *mut libc::c_char);
    }
    mem = kalloc() as *mut libc::c_char;
    memset(mem as *mut libc::c_void, 0 as libc::c_int, PGSIZE as uint);
    mappages(pagetable, 0 as libc::c_int as uint64, PGSIZE as uint64,
             mem as uint64, (PTE_W | PTE_R | PTE_X | PTE_U) as libc::c_int);
    memmove(mem as *mut libc::c_void, src as *const libc::c_void, sz);
}
// Allocate PTEs and physical memory to grow process from oldsz to
// newsz, which need not be page aligned.  Returns new size or 0 on error.
#[no_mangle]
pub unsafe extern "C" fn uvmalloc(mut pagetable: pagetable_t,
                                  mut oldsz: uint64, mut newsz: uint64)
 -> uint64 {
    let mut mem: *mut libc::c_char = 0 as *mut libc::c_char;
    let mut a: uint64 = 0;
    if newsz < oldsz { return oldsz }
    oldsz =
        oldsz.wrapping_add(PGSIZE as
                               libc::c_ulong).wrapping_sub(1 as libc::c_int as
                                                               libc::c_ulong)
            & !(PGSIZE - 1 as libc::c_int) as libc::c_ulong;
    a = oldsz;
    while a < newsz {
        mem = kalloc() as *mut libc::c_char;
        if mem.is_null() {
            uvmdealloc(pagetable, a, oldsz);
            return 0 as libc::c_int as uint64
        }
        memset(mem as *mut libc::c_void, 0 as libc::c_int, PGSIZE as uint);
        if mappages(pagetable, a, PGSIZE as uint64, mem as uint64,
                    (PTE_W | PTE_X | PTE_R | PTE_U) as libc::c_int) !=
               0 as libc::c_int {
            kfree(mem as *mut libc::c_void);
            uvmdealloc(pagetable, a, oldsz);
            return 0 as libc::c_int as uint64
        }
        a =
            (a as libc::c_ulong).wrapping_add(PGSIZE as libc::c_ulong) as
                uint64 as uint64
    }
    return newsz;
}
// Deallocate user pages to bring the process size from oldsz to
// newsz.  oldsz and newsz need not be page-aligned, nor does newsz
// need to be less than oldsz.  oldsz can be larger than the actual
// process size.  Returns the new process size.
#[no_mangle]
pub unsafe extern "C" fn uvmdealloc(mut pagetable: pagetable_t,
                                    mut oldsz: uint64, mut newsz: uint64)
 -> uint64 {
    if newsz >= oldsz { return oldsz }
    let mut newup: uint64 =
        newsz.wrapping_add(PGSIZE as
                               libc::c_ulong).wrapping_sub(1 as libc::c_int as
                                                               libc::c_ulong)
            & !(PGSIZE - 1 as libc::c_int) as libc::c_ulong;
    if newup <
           oldsz.wrapping_add(PGSIZE as
                                  libc::c_ulong).wrapping_sub(1 as libc::c_int
                                                                  as
                                                                  libc::c_ulong)
               & !(PGSIZE - 1 as libc::c_int) as libc::c_ulong {
        uvmunmap(pagetable, newup, oldsz.wrapping_sub(newup),
                 1 as libc::c_int);
    }
    return newsz;
}
// Recursively free page-table pages.
// All leaf mappings must already have been removed.
unsafe extern "C" fn freewalk(mut pagetable: pagetable_t) {
    // there are 2^9 = 512 PTEs in a page table.
    let mut i: libc::c_int = 0 as libc::c_int;
    while i < 512 as libc::c_int {
        let mut pte: pte_t = *pagetable.offset(i as isize);
        if pte & PTE_V as libc::c_ulong != 0 &&
               pte & (PTE_R | PTE_W | PTE_X) as libc::c_ulong ==
                   0 as libc::c_int as libc::c_ulong {
            // this PTE points to a lower-level page table.
            let mut child: uint64 =
                (pte >> 10 as libc::c_int) << 12 as libc::c_int;
            freewalk(child as pagetable_t);
            *pagetable.offset(i as isize) = 0 as libc::c_int as uint64
        } else if pte & PTE_V as libc::c_ulong != 0 {
            panic(b"freewalk: leaf\x00" as *const u8 as *const libc::c_char as
                      *mut libc::c_char);
        }
        i += 1
    }
    kfree(pagetable as *mut libc::c_void);
}
// Free user memory pages,
// then free page-table pages.
#[no_mangle]
pub unsafe extern "C" fn uvmfree(mut pagetable: pagetable_t, mut sz: uint64) {
    uvmunmap(pagetable, 0 as libc::c_int as uint64, sz, 1 as libc::c_int);
    freewalk(pagetable);
}
// Given a parent process's page table, copy
// its memory into a child's page table.
// Copies both the page table and the
// physical memory.
// returns 0 on success, -1 on failure.
// frees any allocated pages on failure.
#[no_mangle]
pub unsafe extern "C" fn uvmcopy(mut old: pagetable_t, mut new: pagetable_t,
                                 mut sz: uint64) -> libc::c_int {
    let mut current_block: u64;
    let mut pte: *mut pte_t = 0 as *mut pte_t;
    let mut pa: uint64 = 0;
    let mut i: uint64 = 0;
    let mut flags: uint = 0;
    let mut mem: *mut libc::c_char = 0 as *mut libc::c_char;
    i = 0 as libc::c_int as uint64;
    loop  {
        if !(i < sz) { current_block = 12349973810996921269; break ; }
        pte = walk(old, i, 0 as libc::c_int);
        if pte.is_null() {
            panic(b"uvmcopy: pte should exist\x00" as *const u8 as
                      *const libc::c_char as *mut libc::c_char);
        }
        if *pte & PTE_V as libc::c_ulong == 0 as libc::c_int as libc::c_ulong
           {
            panic(b"uvmcopy: page not present\x00" as *const u8 as
                      *const libc::c_char as *mut libc::c_char);
        }
        pa = (*pte >> 10 as libc::c_int) << 12 as libc::c_int;
        flags = (*pte & 0x3ff as libc::c_int as libc::c_ulong) as uint;
        mem = kalloc() as *mut libc::c_char;
        if mem.is_null() { current_block = 9000140654394160520; break ; }
        memmove(mem as *mut libc::c_void,
                pa as *mut libc::c_char as *const libc::c_void,
                PGSIZE as uint);
        if mappages(new, i, PGSIZE as uint64, mem as uint64,
                    flags as libc::c_int) != 0 as libc::c_int {
            kfree(mem as *mut libc::c_void);
            current_block = 9000140654394160520;
            break ;
        } else {
            i =
                (i as libc::c_ulong).wrapping_add(PGSIZE as libc::c_ulong) as
                    uint64 as uint64
        }
    }
    match current_block {
        12349973810996921269 => { return 0 as libc::c_int }
        _ => {
            uvmunmap(new, 0 as libc::c_int as uint64, i, 1 as libc::c_int);
            return -(1 as libc::c_int)
        }
    };
}
// mark a PTE invalid for user access.
// used by exec for the user stack guard page.
#[no_mangle]
pub unsafe extern "C" fn uvmclear(mut pagetable: pagetable_t,
                                  mut va: uint64) {
    let mut pte: *mut pte_t = 0 as *mut pte_t;
    pte = walk(pagetable, va, 0 as libc::c_int);
    if pte.is_null() {
        panic(b"uvmclear\x00" as *const u8 as *const libc::c_char as
                  *mut libc::c_char);
    }
    *pte &= !PTE_U as libc::c_ulong;
}
// Copy from kernel to user.
// Copy len bytes from src to virtual address dstva in a given page table.
// Return 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn copyout(mut pagetable: pagetable_t,
                                 mut dstva: uint64,
                                 mut src: *mut libc::c_char, mut len: uint64)
 -> libc::c_int {
    let mut n: uint64 = 0;
    let mut va0: uint64 = 0;
    let mut pa0: uint64 = 0;
    while len > 0 as libc::c_int as libc::c_ulong {
        va0 = dstva & !(PGSIZE - 1 as libc::c_int) as libc::c_ulong;
        pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 as libc::c_int as libc::c_ulong {
            return -(1 as libc::c_int)
        }
        n = (PGSIZE as libc::c_ulong).wrapping_sub(dstva.wrapping_sub(va0));
        if n > len { n = len }
        memmove(pa0.wrapping_add(dstva.wrapping_sub(va0)) as
                    *mut libc::c_void, src as *const libc::c_void, n as uint);
        len = (len as libc::c_ulong).wrapping_sub(n) as uint64 as uint64;
        src = src.offset(n as isize);
        dstva = va0.wrapping_add(PGSIZE as libc::c_ulong)
    }
    return 0 as libc::c_int;
}
// Copy from user to kernel.
// Copy len bytes to dst from virtual address srcva in a given page table.
// Return 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn copyin(mut pagetable: pagetable_t,
                                mut dst: *mut libc::c_char, mut srcva: uint64,
                                mut len: uint64) -> libc::c_int {
    let mut n: uint64 = 0;
    let mut va0: uint64 = 0;
    let mut pa0: uint64 = 0;
    while len > 0 as libc::c_int as libc::c_ulong {
        va0 = srcva & !(PGSIZE - 1 as libc::c_int) as libc::c_ulong;
        pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 as libc::c_int as libc::c_ulong {
            return -(1 as libc::c_int)
        }
        n = (PGSIZE as libc::c_ulong).wrapping_sub(srcva.wrapping_sub(va0));
        if n > len { n = len }
        memmove(dst as *mut libc::c_void,
                pa0.wrapping_add(srcva.wrapping_sub(va0)) as
                    *mut libc::c_void, n as uint);
        len = (len as libc::c_ulong).wrapping_sub(n) as uint64 as uint64;
        dst = dst.offset(n as isize);
        srcva = va0.wrapping_add(PGSIZE as libc::c_ulong)
    }
    return 0 as libc::c_int;
}
// Copy a null-terminated string from user to kernel.
// Copy bytes to dst from virtual address srcva in a given page table,
// until a '\0', or max.
// Return 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn copyinstr(mut pagetable: pagetable_t,
                                   mut dst: *mut libc::c_char,
                                   mut srcva: uint64, mut max: uint64)
 -> libc::c_int {
    let mut n: uint64 = 0;
    let mut va0: uint64 = 0;
    let mut pa0: uint64 = 0;
    let mut got_null: libc::c_int = 0 as libc::c_int;
    while got_null == 0 as libc::c_int &&
              max > 0 as libc::c_int as libc::c_ulong {
        va0 = srcva & !(PGSIZE - 1 as libc::c_int) as libc::c_ulong;
        pa0 = walkaddr(pagetable, va0);
        if pa0 == 0 as libc::c_int as libc::c_ulong {
            return -(1 as libc::c_int)
        }
        n = (PGSIZE as libc::c_ulong).wrapping_sub(srcva.wrapping_sub(va0));
        if n > max { n = max }
        let mut p: *mut libc::c_char =
            pa0.wrapping_add(srcva.wrapping_sub(va0)) as *mut libc::c_char;
        while n > 0 as libc::c_int as libc::c_ulong {
            if *p as libc::c_int == '\u{0}' as i32 {
                *dst = '\u{0}' as i32 as libc::c_char;
                got_null = 1 as libc::c_int;
                break ;
            } else {
                *dst = *p;
                n = n.wrapping_sub(1);
                max = max.wrapping_sub(1);
                p = p.offset(1);
                dst = dst.offset(1)
            }
        }
        srcva = va0.wrapping_add(PGSIZE as libc::c_ulong)
    }
    if got_null != 0 {
        return 0 as libc::c_int
    } else { return -(1 as libc::c_int) };
}
