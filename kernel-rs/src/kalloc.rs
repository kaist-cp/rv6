use crate::libc;
use core::ptr;
use crate::printf::printf;
extern "C" {
    pub type cpu;
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    // spinlock.c
    #[no_mangle]
    fn acquire(_: *mut spinlock);
    #[no_mangle]
    fn initlock(_: *mut spinlock, _: *mut libc::c_char);
    #[no_mangle]
    fn release(_: *mut spinlock);
    #[no_mangle]
    fn memset(_: *mut libc::c_void, _: libc::c_int, _: uint) -> *mut libc::c_void;
}
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;

pub static mut end: [u8;0] = [0; 0];
// Mutual exclusion lock.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct spinlock {
    pub locked: uint,
    pub name: *mut libc::c_char,
    pub cpu: *mut cpu,
}
// first address after kernel.
// defined by kernel.ld.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct run {
    pub next: *mut run,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct C2RustUnnamed {
    pub lock: spinlock,
    pub freelist: *mut run,
}
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
// virtio mmio interface
// local interrupt controller, which contains the timer.
// cycles since boot.
// qemu puts programmable interrupt controller here.
// the kernel expects there to be RAM
// for use by the kernel and user pages
// from physical address 0x80000000 to PHYSTOP.
pub const KERNBASE: libc::c_long = 0x80000000 as libc::c_long;
pub const PHYSTOP: libc::c_long =
    KERNBASE + (128 as libc::c_int * 1024 as libc::c_int * 1024 as libc::c_int) as libc::c_long;
pub const PGSIZE: libc::c_int = 4096 as libc::c_int;
#[no_mangle]
pub static mut kmem: C2RustUnnamed = C2RustUnnamed {
    lock: spinlock {
        locked: 0,
        name: 0 as *const libc::c_char as *mut libc::c_char,
        cpu: 0 as *const cpu as *mut cpu,
    },
    freelist: 0 as *const run as *mut run,
};
#[no_mangle]
pub unsafe extern "C" fn kinit() {
    initlock(
        &mut kmem.lock,
        b"kmem\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    let a = 10;
    printf(b"\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    printf(b"\x00" as *const u8 as *const libc::c_char as *mut libc::c_char, a);
    freerange(
        end.as_mut_ptr() as *mut libc::c_void,
        PHYSTOP as *mut libc::c_void,
    );
}
// Physical memory allocator, for user processes,
// kernel stacks, page-table pages,
// and pipe buffers. Allocates whole 4096-byte pages.
#[no_mangle]
pub unsafe extern "C" fn freerange(mut pa_start: *mut libc::c_void, mut pa_end: *mut libc::c_void) {
    let mut p: *mut libc::c_char = ptr::null_mut();
    p = ((pa_start as uint64)
        .wrapping_add(PGSIZE as libc::c_ulong)
        .wrapping_sub(1 as libc::c_int as libc::c_ulong)
        & !(PGSIZE - 1 as libc::c_int) as libc::c_ulong) as *mut libc::c_char;
    
    while p.offset(PGSIZE as isize) <= pa_end as *mut libc::c_char {
        kfree(p as *mut libc::c_void);
        p = p.offset(PGSIZE as isize)
    }
}
// Free the page of physical memory pointed at by v,
// which normally should have been returned by a
// call to kalloc().  (The exception is when
// initializing the allocator; see kinit above.)
#[no_mangle]
pub unsafe extern "C" fn kfree(mut pa: *mut libc::c_void) {
    let mut r: *mut run = ptr::null_mut();
    if (pa as uint64).wrapping_rem(PGSIZE as libc::c_ulong) != 0 as libc::c_int as libc::c_ulong
        || (pa as *mut libc::c_char) < end.as_mut_ptr()
        || pa as uint64 >= PHYSTOP as libc::c_ulong
    {
        panic(b"kfree\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    // Fill with junk to catch dangling refs.
    memset(pa, 1 as libc::c_int, PGSIZE as uint);
    r = pa as *mut run;
    acquire(&mut kmem.lock);
    (*r).next = kmem.freelist;
    kmem.freelist = r;
    release(&mut kmem.lock);
}
// kalloc.c
// Allocate one 4096-byte page of physical memory.
// Returns a pointer that the kernel can use.
// Returns 0 if the memory cannot be allocated.
#[no_mangle]
pub unsafe extern "C" fn kalloc() -> *mut libc::c_void {
    let mut r: *mut run = ptr::null_mut(); // fill with junk
    acquire(&mut kmem.lock);
    r = kmem.freelist;
    if !r.is_null() {
        kmem.freelist = (*r).next
    }
    release(&mut kmem.lock);
    if !r.is_null() {
        memset(
            r as *mut libc::c_char as *mut libc::c_void,
            5 as libc::c_int,
            PGSIZE as uint,
        );
    }
    r as *mut libc::c_void
}
