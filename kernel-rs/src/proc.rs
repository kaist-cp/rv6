use crate::libc;
use core::ptr;
extern "C" {
    pub type inode;
    pub type file;
    #[no_mangle]
    fn fileclose(_: *mut file);
    #[no_mangle]
    fn filedup(_: *mut file) -> *mut file;
    // fs.c
    #[no_mangle]
    fn fsinit(_: libc::c_int);
    #[no_mangle]
    fn idup(_: *mut inode) -> *mut inode;
    #[no_mangle]
    fn iput(_: *mut inode);
    #[no_mangle]
    fn namei(_: *mut libc::c_char) -> *mut inode;
    // kalloc.c
    #[no_mangle]
    fn kalloc() -> *mut libc::c_void;
    #[no_mangle]
    fn kfree(_: *mut libc::c_void);
    #[no_mangle]
    fn begin_op();
    #[no_mangle]
    fn end_op();
    // printf.c
    #[no_mangle]
    fn printf(_: *mut libc::c_char, _: ...);
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    // swtch.S
    #[no_mangle]
    fn swtch(_: *mut context, _: *mut context);
    // spinlock.c
    #[no_mangle]
    fn acquire(_: *mut spinlock);
    #[no_mangle]
    fn holding(_: *mut spinlock) -> libc::c_int;
    #[no_mangle]
    fn initlock(_: *mut spinlock, _: *mut libc::c_char);
    #[no_mangle]
    fn release(_: *mut spinlock);
    #[no_mangle]
    fn push_off();
    #[no_mangle]
    fn pop_off();
    #[no_mangle]
    fn memmove(_: *mut libc::c_void, _: *const libc::c_void, _: uint) -> *mut libc::c_void;
    #[no_mangle]
    fn memset(_: *mut libc::c_void, _: libc::c_int, _: uint) -> *mut libc::c_void;
    #[no_mangle]
    fn safestrcpy(
        _: *mut libc::c_char,
        _: *const libc::c_char,
        _: libc::c_int,
    ) -> *mut libc::c_char;
    #[no_mangle]
    fn usertrapret();
    #[no_mangle]
    fn kvminithart();
    #[no_mangle]
    fn kvmmap(_: uint64, _: uint64, _: uint64, _: libc::c_int);
    #[no_mangle]
    fn mappages(_: pagetable_t, _: uint64, _: uint64, _: uint64, _: libc::c_int) -> libc::c_int;
    #[no_mangle]
    fn uvmcreate() -> pagetable_t;
    #[no_mangle]
    fn uvminit(_: pagetable_t, _: *mut uchar, _: uint);
    #[no_mangle]
    fn uvmalloc(_: pagetable_t, _: uint64, _: uint64) -> uint64;
    #[no_mangle]
    fn uvmdealloc(_: pagetable_t, _: uint64, _: uint64) -> uint64;
    #[no_mangle]
    fn uvmcopy(_: pagetable_t, _: pagetable_t, _: uint64) -> libc::c_int;
    #[no_mangle]
    fn uvmfree(_: pagetable_t, _: uint64);
    #[no_mangle]
    fn uvmunmap(_: pagetable_t, _: uint64, _: uint64, _: libc::c_int);
    #[no_mangle]
    fn copyout(_: pagetable_t, _: uint64, _: *mut libc::c_char, _: uint64) -> libc::c_int;
    #[no_mangle]
    fn copyin(_: pagetable_t, _: *mut libc::c_char, _: uint64, _: uint64) -> libc::c_int;
    #[no_mangle]
    static mut trampoline: [libc::c_char; 0];

    #[no_mangle]
    static mut cpus: [cpu; 8];
    #[no_mangle]
    static mut proc: [proc_0; 64];
    #[no_mangle]
    static mut initproc: *mut proc_0;
}
pub type uint = libc::c_uint;
pub type uchar = libc::c_uchar;
pub type uint64 = libc::c_ulong;
pub type pagetable_t = *mut uint64;
// Mutual exclusion lock.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct spinlock {
    pub locked: uint,
    pub name: *mut libc::c_char,
    pub cpu: *mut cpu,
}
// Per-CPU state.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct cpu {
    pub proc_0: *mut proc_0,
    pub scheduler: context,
    pub noff: libc::c_int,
    pub intena: libc::c_int,
}
// Saved registers for kernel context switches.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct context {
    pub ra: uint64,
    pub sp: uint64,
    pub s0: uint64,
    pub s1: uint64,
    pub s2: uint64,
    pub s3: uint64,
    pub s4: uint64,
    pub s5: uint64,
    pub s6: uint64,
    pub s7: uint64,
    pub s8: uint64,
    pub s9: uint64,
    pub s10: uint64,
    pub s11: uint64,
}
// Per-process state
#[derive(Copy, Clone)]
#[repr(C)]
pub struct proc_0 {
    pub lock: spinlock,
    pub state: procstate,
    pub parent: *mut proc_0,
    pub chan: *mut libc::c_void,
    pub killed: libc::c_int,
    pub xstate: libc::c_int,
    pub pid: libc::c_int,
    pub kstack: uint64,
    pub sz: uint64,
    pub pagetable: pagetable_t,
    pub tf: *mut trapframe,
    pub context: context,
    pub ofile: [*mut file; 16],
    pub cwd: *mut inode,
    pub name: [libc::c_char; 16],
}
// Were interrupts enabled before push_off()?
// per-process data for the trap handling code in trampoline.S.
// sits in a page by itself just under the trampoline page in the
// user page table. not specially mapped in the kernel page table.
// the sscratch register points here.
// uservec in trampoline.S saves user registers in the trapframe,
// then initializes registers from the trapframe's
// kernel_sp, kernel_hartid, kernel_satp, and jumps to kernel_trap.
// usertrapret() and userret in trampoline.S set up
// the trapframe's kernel_*, restore user registers from the
// trapframe, switch to the user page table, and enter user space.
// the trapframe includes callee-saved user registers like s0-s11 because the
// return-to-user path via usertrapret() doesn't return through
// the entire kernel call stack.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct trapframe {
    pub kernel_satp: uint64,
    pub kernel_sp: uint64,
    pub kernel_trap: uint64,
    pub epc: uint64,
    pub kernel_hartid: uint64,
    pub ra: uint64,
    pub sp: uint64,
    pub gp: uint64,
    pub tp: uint64,
    pub t0: uint64,
    pub t1: uint64,
    pub t2: uint64,
    pub s0: uint64,
    pub s1: uint64,
    pub a0: uint64,
    pub a1: uint64,
    pub a2: uint64,
    pub a3: uint64,
    pub a4: uint64,
    pub a5: uint64,
    pub a6: uint64,
    pub a7: uint64,
    pub s2: uint64,
    pub s3: uint64,
    pub s4: uint64,
    pub s5: uint64,
    pub s6: uint64,
    pub s7: uint64,
    pub s8: uint64,
    pub s9: uint64,
    pub s10: uint64,
    pub s11: uint64,
    pub t3: uint64,
    pub t4: uint64,
    pub t5: uint64,
    pub t6: uint64,
}
pub type procstate = libc::c_uint;
pub const ZOMBIE: procstate = 4;
pub const RUNNING: procstate = 3;
pub const RUNNABLE: procstate = 2;
pub const SLEEPING: procstate = 1;
pub const UNUSED: procstate = 0;
pub const NPROC: libc::c_int = 64 as libc::c_int;
// maximum number of processes
// maximum number of CPUs
pub const NOFILE: libc::c_int = 16 as libc::c_int;
// open files per process
// open files per system
// maximum number of active i-nodes
// maximum major device number
pub const ROOTDEV: libc::c_int = 1 as libc::c_int;
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
// map the trampoline page to the highest address,
// in both user and kernel space.
pub const TRAMPOLINE: libc::c_long = MAXVA - PGSIZE as libc::c_long;
// map kernel stacks beneath the trampoline,
// each surrounded by invalid guard pages.
// User memory layout.
// Address zero first:
//   text
//   original data and bss
//   fixed-size stack
//   expandable heap
//   ...
//   TRAPFRAME (p->tf, used by the trampoline)
//   TRAMPOLINE (the same page as in the kernel)
pub const TRAPFRAME: libc::c_long = TRAMPOLINE - PGSIZE as libc::c_long;
// Supervisor Status Register, sstatus
// Previous mode, 1=Supervisor, 0=User
// Supervisor Previous Interrupt Enable
// User Previous Interrupt Enable
pub const SSTATUS_SIE: libc::c_long = (1 as libc::c_long) << 1 as libc::c_int;
// Supervisor Interrupt Enable
// User Interrupt Enable
#[inline]
unsafe extern "C" fn r_sstatus() -> uint64 {
    let mut x: uint64 = 0;
    llvm_asm!("csrr $0, sstatus" : "=r" (x) : : : "volatile");
    x
}
#[inline]
unsafe extern "C" fn w_sstatus(mut x: uint64) {
    llvm_asm!("csrw sstatus, $0" : : "r" (x) : : "volatile");
}
// Supervisor Interrupt Enable
pub const SIE_SEIE: libc::c_long = (1 as libc::c_long) << 9 as libc::c_int;
// external
pub const SIE_STIE: libc::c_long = (1 as libc::c_long) << 5 as libc::c_int;
// timer
pub const SIE_SSIE: libc::c_long = (1 as libc::c_long) << 1 as libc::c_int;
// software
#[inline]
unsafe extern "C" fn r_sie() -> uint64 {
    let mut x: uint64 = 0;
    llvm_asm!("csrr $0, sie" : "=r" (x) : : : "volatile");
    x
}
#[inline]
unsafe extern "C" fn w_sie(mut x: uint64) {
    llvm_asm!("csrw sie, $0" : : "r" (x) : : "volatile");
}
// enable device interrupts
#[inline]
unsafe extern "C" fn intr_on() {
    w_sie(
        r_sie() | SIE_SEIE as libc::c_ulong | SIE_STIE as libc::c_ulong | SIE_SSIE as libc::c_ulong,
    );
    w_sstatus(r_sstatus() | SSTATUS_SIE as libc::c_ulong);
}
// are device interrupts enabled?
#[inline]
unsafe extern "C" fn intr_get() -> libc::c_int {
    let mut x: uint64 = r_sstatus();
    (x & SSTATUS_SIE as libc::c_ulong != 0 as libc::c_int as libc::c_ulong) as libc::c_int
}
// read and write tp, the thread pointer, which holds
// this core's hartid (core number), the index into cpus[].
#[inline]
unsafe extern "C" fn r_tp() -> uint64 {
    let mut x: uint64 = 0;
    llvm_asm!("mv $0, tp" : "=r" (x) : : : "volatile");
    x
}
pub const PGSIZE: libc::c_int = 4096 as libc::c_int;
// bytes per page
// bits of offset within a page
// valid
pub const PTE_R: libc::c_long = (1 as libc::c_long) << 1 as libc::c_int;
pub const PTE_W: libc::c_long = (1 as libc::c_long) << 2 as libc::c_int;
pub const PTE_X: libc::c_long = (1 as libc::c_long) << 3 as libc::c_int;
// 1 -> user can access
// shift a physical address to the right place for a PTE.
// extract the three 9-bit page table indices from a virtual address.
// 9 bits
// one beyond the highest possible virtual address.
// MAXVA is actually one bit less than the max allowed by
// Sv39, to avoid having to sign-extend virtual addresses
// that have the high bit set.
pub const MAXVA: libc::c_long = (1 as libc::c_long)
    << (9 as libc::c_int + 9 as libc::c_int + 9 as libc::c_int + 12 as libc::c_int
        - 1 as libc::c_int);
// #[no_mangle]
// pub static mut cpus: [cpu; 8] = [cpu {
//     proc_0: ptr::null_mut(), //0 as *const proc_ptr::null_mut(),
//     scheduler: context {
//         ra: 0,
//         sp: 0,
//         s0: 0,
//         s1: 0,
//         s2: 0,
//         s3: 0,
//         s4: 0,
//         s5: 0,
//         s6: 0,
//         s7: 0,
//         s8: 0,
//         s9: 0,
//         s10: 0,
//         s11: 0,
//     },
//     noff: 0,
//     intena: 0,
// }; 8];
// #[export_name = "proc"]
// pub static mut proc_0: [proc_0; 64] = [proc_0 {
//     lock: spinlock {
//         locked: 0,
//         name: 0 as *const libc::c_char as *mut libc::c_char,
//         cpu: 0 as *const cpu as *mut cpu,
//     },
//     state: UNUSED,
//     parent: 0 as *const proc_ptr::null_mut(),
//     chan: 0 as *const libc::c_void as *mut libc::c_void,
//     killed: 0,
//     xstate: 0,
//     pid: 0,
//     kstack: 0,
//     sz: 0,
//     pagetable: 0 as *const uint64 as *mut uint64,
//     tf: 0 as *const trapframe as *mut trapframe,
//     context: context {
//         ra: 0,
//         sp: 0,
//         s0: 0,
//         s1: 0,
//         s2: 0,
//         s3: 0,
//         s4: 0,
//         s5: 0,
//         s6: 0,
//         s7: 0,
//         s8: 0,
//         s9: 0,
//         s10: 0,
//         s11: 0,
//     },
//     ofile: [0 as *const file as *mut file; 16],
//     cwd: 0 as *const inode as *mut inode,
//     name: [0; 16],
// }; 64];
// #[no_mangle]
// pub static mut initproc: *mut proc_0 = ptr::null_mut();
// pub static mut initproc: *mut proc_0 = 0 as *const proc_ptr::null_mut();
#[no_mangle]
pub static mut nextpid: libc::c_int = 1 as libc::c_int;
#[no_mangle]
pub static mut pid_lock: spinlock = spinlock {
    locked: 0,
    name: 0 as *const libc::c_char as *mut libc::c_char,
    cpu: 0 as *const cpu as *mut cpu,
};
// trampoline.S
#[no_mangle]
pub unsafe extern "C" fn procinit() {
    let mut p: *mut proc_0 = ptr::null_mut();
    initlock(
        &mut pid_lock,
        b"nextpid\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    p = proc.as_mut_ptr();
    while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
        initlock(
            &mut (*p).lock,
            b"proc\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        );
        // Allocate a page for the process's kernel stack.
        // Map it high in memory, followed by an invalid
        // guard page.
        let mut pa: *mut libc::c_char = kalloc() as *mut libc::c_char;
        if pa.is_null() {
            panic(b"kalloc\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        let mut va: uint64 = (TRAMPOLINE
            - ((p.wrapping_offset_from(proc.as_mut_ptr()) as libc::c_long as libc::c_int
                + 1 as libc::c_int)
                * 2 as libc::c_int
                * PGSIZE) as libc::c_long) as uint64;
        kvmmap(
            va,
            pa as uint64,
            PGSIZE as uint64,
            (PTE_R | PTE_W) as libc::c_int,
        );
        (*p).kstack = va;
        p = p.offset(1)
    }
    kvminithart();
}
// proc.c
// Must be called with interrupts disabled,
// to prevent race with process being moved
// to a different CPU.
#[no_mangle]
pub unsafe extern "C" fn cpuid() -> libc::c_int {
    let mut id: libc::c_int = r_tp() as libc::c_int;
    id
}
// Return this CPU's cpu struct.
// Interrupts must be disabled.
#[no_mangle]
pub unsafe extern "C" fn mycpu() -> *mut cpu {
    let mut id: libc::c_int = cpuid();
    let mut c: *mut cpu = &mut *cpus.as_mut_ptr().offset(id as isize) as *mut cpu;
    c
}
// Return the current struct proc *, or zero if none.
#[no_mangle]
pub unsafe extern "C" fn myproc() -> *mut proc_0 {
    push_off();
    let mut c: *mut cpu = mycpu();
    let mut p: *mut proc_0 = (*c).proc_0;
    pop_off();
    p
}
#[no_mangle]
pub unsafe extern "C" fn allocpid() -> libc::c_int {
    let mut pid: libc::c_int = 0;
    acquire(&mut pid_lock);
    pid = nextpid;
    nextpid += 1;
    release(&mut pid_lock);
    pid
}
// Look in the process table for an UNUSED proc.
// If found, initialize state required to run in the kernel,
// and return with p->lock held.
// If there are no free procs, return 0.
unsafe extern "C" fn allocproc() -> *mut proc_0 {
    let mut current_block: u64;
    let mut p: *mut proc_0 = ptr::null_mut();
    p = proc.as_mut_ptr();
    loop {
        if p >= &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
            current_block = 7815301370352969686;
            break;
        }
        acquire(&mut (*p).lock);
        if (*p).state as libc::c_uint == UNUSED as libc::c_int as libc::c_uint {
            current_block = 17234009953499979309;
            break;
        }
        release(&mut (*p).lock);
        p = p.offset(1)
    }
    match current_block {
        7815301370352969686 => ptr::null_mut(),
        _ => {
            (*p).pid = allocpid();
            // Allocate a trapframe page.
            (*p).tf = kalloc() as *mut trapframe;
            if (*p).tf.is_null() {
                release(&mut (*p).lock);
                return ptr::null_mut();
            }
            // An empty user page table.
            (*p).pagetable = proc_pagetable(p);
            // Set up new context to start executing at forkret,
            // which returns to user space.
            memset(
                &mut (*p).context as *mut context as *mut libc::c_void,
                0 as libc::c_int,
                ::core::mem::size_of::<context>() as libc::c_ulong as uint,
            );
            (*p).context.ra = ::core::mem::transmute::<Option<unsafe extern "C" fn() -> ()>, uint64>(
                Some(forkret as unsafe extern "C" fn() -> ()),
            );
            (*p).context.sp = (*p).kstack.wrapping_add(PGSIZE as libc::c_ulong);
            p
        }
    }
}
// free a proc structure and the data hanging from it,
// including user pages.
// p->lock must be held.
unsafe extern "C" fn freeproc(mut p: *mut proc_0) {
    if !(*p).tf.is_null() {
        kfree((*p).tf as *mut libc::c_void);
    }
    (*p).tf = ptr::null_mut();
    if !(*p).pagetable.is_null() {
        proc_freepagetable((*p).pagetable, (*p).sz);
    }
    (*p).pagetable = 0 as pagetable_t;
    (*p).sz = 0 as libc::c_int as uint64;
    (*p).pid = 0 as libc::c_int;
    (*p).parent = ptr::null_mut();
    (*p).name[0 as libc::c_int as usize] = 0 as libc::c_int as libc::c_char;
    (*p).chan = ptr::null_mut();
    (*p).killed = 0 as libc::c_int;
    (*p).xstate = 0 as libc::c_int;
    (*p).state = UNUSED;
}
// Create a page table for a given process,
// with no user pages, but with trampoline pages.
#[no_mangle]
pub unsafe extern "C" fn proc_pagetable(mut p: *mut proc_0) -> pagetable_t {
    let mut pagetable: pagetable_t = ptr::null_mut();
    // An empty page table.
    pagetable = uvmcreate();
    // map the trampoline code (for system call return)
    // at the highest user virtual address.
    // only the supervisor uses it, on the way
    // to/from user space, so not PTE_U.
    mappages(
        pagetable,
        TRAMPOLINE as uint64,
        PGSIZE as uint64,
        trampoline.as_mut_ptr() as uint64,
        (PTE_R | PTE_X) as libc::c_int,
    );
    // map the trapframe just below TRAMPOLINE, for trampoline.S.
    mappages(
        pagetable,
        TRAPFRAME as uint64,
        PGSIZE as uint64,
        (*p).tf as uint64,
        (PTE_R | PTE_W) as libc::c_int,
    );
    pagetable
}
// Free a process's page table, and free the
// physical memory it refers to.
#[no_mangle]
pub unsafe extern "C" fn proc_freepagetable(mut pagetable: pagetable_t, mut sz: uint64) {
    uvmunmap(
        pagetable,
        TRAMPOLINE as uint64,
        PGSIZE as uint64,
        0 as libc::c_int,
    );
    uvmunmap(
        pagetable,
        TRAPFRAME as uint64,
        PGSIZE as uint64,
        0 as libc::c_int,
    );
    if sz > 0 as libc::c_int as libc::c_ulong {
        uvmfree(pagetable, sz);
    };
}
// a user program that calls exec("/init")
// od -t xC initcode
#[no_mangle]
pub static mut initcode: [uchar; 51] = [
    0x17 as libc::c_int as uchar,
    0x5 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0x13 as libc::c_int as uchar,
    0x5 as libc::c_int as uchar,
    0x5 as libc::c_int as uchar,
    0x2 as libc::c_int as uchar,
    0x97 as libc::c_int as uchar,
    0x5 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0x93 as libc::c_int as uchar,
    0x85 as libc::c_int as uchar,
    0x5 as libc::c_int as uchar,
    0x2 as libc::c_int as uchar,
    0x9d as libc::c_int as uchar,
    0x48 as libc::c_int as uchar,
    0x73 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0x89 as libc::c_int as uchar,
    0x48 as libc::c_int as uchar,
    0x73 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0xef as libc::c_int as uchar,
    0xf0 as libc::c_int as uchar,
    0xbf as libc::c_int as uchar,
    0xff as libc::c_int as uchar,
    0x2f as libc::c_int as uchar,
    0x69 as libc::c_int as uchar,
    0x6e as libc::c_int as uchar,
    0x69 as libc::c_int as uchar,
    0x74 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0x1 as libc::c_int as uchar,
    0x20 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
    0 as libc::c_int as uchar,
];
// Set up first user process.
#[no_mangle]
pub unsafe extern "C" fn userinit() {
    let mut p: *mut proc_0 = ptr::null_mut();
    p = allocproc();
    initproc = p;
    // allocate one user page and copy init's instructions
    // and data into it.
    uvminit(
        (*p).pagetable,
        initcode.as_mut_ptr(),
        ::core::mem::size_of::<[uchar; 51]>() as libc::c_ulong as uint,
    );
    (*p).sz = PGSIZE as uint64;
    // prepare for the very first "return" from kernel to user.
    (*(*p).tf).epc = 0 as libc::c_int as uint64; // user program counter
    (*(*p).tf).sp = PGSIZE as uint64; // user stack pointer
    safestrcpy(
        (*p).name.as_mut_ptr(),
        b"initcode\x00" as *const u8 as *const libc::c_char,
        ::core::mem::size_of::<[libc::c_char; 16]>() as libc::c_ulong as libc::c_int,
    );
    (*p).cwd = namei(b"/\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    (*p).state = RUNNABLE;
    release(&mut (*p).lock);
}
// Grow or shrink user memory by n bytes.
// Return 0 on success, -1 on failure.
#[no_mangle]
pub unsafe extern "C" fn growproc(mut n: libc::c_int) -> libc::c_int {
    let mut sz: uint = 0;
    let mut p: *mut proc_0 = myproc();
    sz = (*p).sz as uint;
    if n > 0 as libc::c_int {
        sz = uvmalloc(
            (*p).pagetable,
            sz as uint64,
            sz.wrapping_add(n as libc::c_uint) as uint64,
        ) as uint;
        if sz == 0 as libc::c_int as libc::c_uint {
            return -(1 as libc::c_int);
        }
    } else if n < 0 as libc::c_int {
        sz = uvmdealloc(
            (*p).pagetable,
            sz as uint64,
            sz.wrapping_add(n as libc::c_uint) as uint64,
        ) as uint
    }
    (*p).sz = sz as uint64;
    0 as libc::c_int
}
// Create a new process, copying the parent.
// Sets up child kernel stack to return as if from fork() system call.
#[no_mangle]
pub unsafe extern "C" fn fork() -> libc::c_int {
    let mut i: libc::c_int = 0;
    let mut pid: libc::c_int = 0;
    let mut np: *mut proc_0 = ptr::null_mut();
    let mut p: *mut proc_0 = myproc();
    // Allocate process.
    np = allocproc();
    if np.is_null() {
        return -(1 as libc::c_int);
    }
    // Copy user memory from parent to child.
    if uvmcopy((*p).pagetable, (*np).pagetable, (*p).sz) < 0 as libc::c_int {
        freeproc(np);
        release(&mut (*np).lock);
        return -(1 as libc::c_int);
    }
    (*np).sz = (*p).sz;
    (*np).parent = p;
    // copy saved user registers.
    *(*np).tf = *(*p).tf;
    // Cause fork to return 0 in the child.
    (*(*np).tf).a0 = 0 as libc::c_int as uint64;
    // increment reference counts on open file descriptors.
    i = 0 as libc::c_int;
    while i < NOFILE {
        if !(*p).ofile[i as usize].is_null() {
            (*np).ofile[i as usize] = filedup((*p).ofile[i as usize])
        }
        i += 1
    }
    (*np).cwd = idup((*p).cwd);
    safestrcpy(
        (*np).name.as_mut_ptr(),
        (*p).name.as_mut_ptr(),
        ::core::mem::size_of::<[libc::c_char; 16]>() as libc::c_ulong as libc::c_int,
    );
    pid = (*np).pid;
    (*np).state = RUNNABLE;
    release(&mut (*np).lock);
    pid
}
// Pass p's abandoned children to init.
// Caller must hold p->lock.
#[no_mangle]
pub unsafe extern "C" fn reparent(mut p: *mut proc_0) {
    let mut pp: *mut proc_0 = ptr::null_mut();
    pp = proc.as_mut_ptr();
    while pp < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
        // this code uses pp->parent without holding pp->lock.
        // acquiring the lock first could cause a deadlock
        // if pp or a child of pp were also in exit()
        // and about to try to lock p.
        if (*pp).parent == p {
            // pp->parent can't change between the check and the acquire()
            // because only the parent changes it, and we're the parent.
            acquire(&mut (*pp).lock);
            (*pp).parent = initproc;
            // we should wake up init here, but that would require
            // initproc->lock, which would be a deadlock, since we hold
            // the lock on one of init's children (pp). this is why
            // exit() always wakes init (before acquiring any locks).
            release(&mut (*pp).lock);
        }
        pp = pp.offset(1)
    }
}
// Exit the current process.  Does not return.
// An exited process remains in the zombie state
// until its parent calls wait().
#[no_mangle]
pub unsafe extern "C" fn exit(mut status: libc::c_int) {
    let mut p: *mut proc_0 = myproc();
    if p == initproc {
        panic(b"init exiting\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    // Close all open files.
    let mut fd: libc::c_int = 0 as libc::c_int;
    while fd < NOFILE {
        if !(*p).ofile[fd as usize].is_null() {
            let mut f: *mut file = (*p).ofile[fd as usize];
            fileclose(f);
            (*p).ofile[fd as usize] = 0 as *mut file
        }
        fd += 1
    }
    begin_op();
    iput((*p).cwd);
    end_op();
    (*p).cwd = 0 as *mut inode;
    // we might re-parent a child to init. we can't be precise about
    // waking up init, since we can't acquire its lock once we've
    // acquired any other proc lock. so wake up init whether that's
    // necessary or not. init may miss this wakeup, but that seems
    // harmless.
    acquire(&mut (*initproc).lock);
    wakeup1(initproc);
    release(&mut (*initproc).lock);
    // grab a copy of p->parent, to ensure that we unlock the same
    // parent we locked. in case our parent gives us away to init while
    // we're waiting for the parent lock. we may then race with an
    // exiting parent, but the result will be a harmless spurious wakeup
    // to a dead or wrong process; proc structs are never re-allocated
    // as anything else.
    acquire(&mut (*p).lock);
    let mut original_parent: *mut proc_0 = (*p).parent;
    release(&mut (*p).lock);
    // we need the parent's lock in order to wake it up from wait().
    // the parent-then-child rule says we have to lock it first.
    acquire(&mut (*original_parent).lock);
    acquire(&mut (*p).lock);
    // Give any children to init.
    reparent(p);
    // Parent might be sleeping in wait().
    wakeup1(original_parent);
    (*p).xstate = status;
    (*p).state = ZOMBIE;
    release(&mut (*original_parent).lock);
    // Jump into the scheduler, never to return.
    sched();
    panic(b"zombie exit\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
}
// Wait for a child process to exit and return its pid.
// Return -1 if this process has no children.
#[no_mangle]
pub unsafe extern "C" fn wait(mut addr: uint64) -> libc::c_int {
    let mut np: *mut proc_0 = ptr::null_mut();
    let mut havekids: libc::c_int = 0;
    let mut pid: libc::c_int = 0;
    let mut p: *mut proc_0 = myproc();
    // hold p->lock for the whole time to avoid lost
    // wakeups from a child's exit().
    acquire(&mut (*p).lock);
    loop {
        // Scan through table looking for exited children.
        havekids = 0 as libc::c_int;
        np = proc.as_mut_ptr();
        while np < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
            //DOC: wait-sleep
            // this code uses np->parent without holding np->lock.
            // acquiring the lock first would cause a deadlock,
            // since np might be an ancestor, and we already hold p->lock.
            if (*np).parent == p {
                // np->parent can't change between the check and the acquire()
                // because only the parent changes it, and we're the parent.
                acquire(&mut (*np).lock);
                havekids = 1 as libc::c_int;
                if (*np).state as libc::c_uint == ZOMBIE as libc::c_int as libc::c_uint {
                    // Found one.
                    pid = (*np).pid;
                    if addr != 0 as libc::c_int as libc::c_ulong
                        && copyout(
                            (*p).pagetable,
                            addr,
                            &mut (*np).xstate as *mut libc::c_int as *mut libc::c_char,
                            ::core::mem::size_of::<libc::c_int>() as libc::c_ulong,
                        ) < 0 as libc::c_int
                    {
                        release(&mut (*np).lock);
                        release(&mut (*p).lock);
                        return -(1 as libc::c_int);
                    }
                    freeproc(np);
                    release(&mut (*np).lock);
                    release(&mut (*p).lock);
                    return pid;
                }
                release(&mut (*np).lock);
            }
            np = np.offset(1)
        }
        if havekids == 0 || (*p).killed != 0 {
            release(&mut (*p).lock);
            return -(1 as libc::c_int);
        }
        sleep(p as *mut libc::c_void, &mut (*p).lock);
    }
}
// No point waiting if we don't have any children.
// Wait for a child to exit.
// Per-CPU process scheduler.
// Each CPU calls scheduler() after setting itself up.
// Scheduler never returns.  It loops, doing:
//  - choose a process to run.
//  - swtch to start running that process.
//  - eventually that process transfers control
//    via swtch back to the scheduler.
#[no_mangle]
pub unsafe extern "C" fn scheduler() -> ! {
    let mut p: *mut proc_0 = ptr::null_mut();
    let mut c: *mut cpu = mycpu();
    (*c).proc_0 = ptr::null_mut();
    loop {
        // Avoid deadlock by ensuring that devices can interrupt.
        intr_on();
        p = proc.as_mut_ptr();
        while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
            acquire(&mut (*p).lock);
            if (*p).state as libc::c_uint == RUNNABLE as libc::c_int as libc::c_uint {
                // Switch to chosen process.  It is the process's job
                // to release its lock and then reacquire it
                // before jumping back to us.
                (*p).state = RUNNING;
                (*c).proc_0 = p;
                swtch(&mut (*c).scheduler, &mut (*p).context);
                // Process is done running for now.
                // It should have changed its p->state before coming back.
                (*c).proc_0 = ptr::null_mut()
            }
            release(&mut (*p).lock);
            p = p.offset(1)
        }
    }
}
// Switch to scheduler.  Must hold only p->lock
// and have changed proc->state. Saves and restores
// intena because intena is a property of this
// kernel thread, not this CPU. It should
// be proc->intena and proc->noff, but that would
// break in the few places where a lock is held but
// there's no process.
#[no_mangle]
pub unsafe extern "C" fn sched() {
    let mut intena: libc::c_int = 0;
    let mut p: *mut proc_0 = myproc();
    if holding(&mut (*p).lock) == 0 {
        panic(b"sched p->lock\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if (*mycpu()).noff != 1 as libc::c_int {
        panic(b"sched locks\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if (*p).state as libc::c_uint == RUNNING as libc::c_int as libc::c_uint {
        panic(b"sched running\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if intr_get() != 0 {
        panic(b"sched interruptible\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    intena = (*mycpu()).intena;
    swtch(
        &mut (*p).context,
        &mut (*(mycpu as unsafe extern "C" fn() -> *mut cpu)()).scheduler,
    );
    (*mycpu()).intena = intena;
}
// Give up the CPU for one scheduling round.
#[export_name = "yield"]
pub unsafe extern "C" fn yield_0() {
    let mut p: *mut proc_0 = myproc();
    acquire(&mut (*p).lock);
    (*p).state = RUNNABLE;
    sched();
    release(&mut (*p).lock);
}
// A fork child's very first scheduling by scheduler()
// will swtch to forkret.
#[no_mangle]
pub unsafe extern "C" fn forkret() {
    static mut first: libc::c_int = 1 as libc::c_int;
    // Still holding p->lock from scheduler.
    release(&mut (*(myproc as unsafe extern "C" fn() -> *mut proc_0)()).lock);
    if first != 0 {
        // File system initialization must be run in the context of a
        // regular process (e.g., because it calls sleep), and thus cannot
        // be run from main().
        first = 0 as libc::c_int;
        fsinit(ROOTDEV);
    }
    usertrapret();
}
// Atomically release lock and sleep on chan.
// Reacquires lock when awakened.
#[no_mangle]
pub unsafe extern "C" fn sleep(mut chan: *mut libc::c_void, mut lk: *mut spinlock) {
    let mut p: *mut proc_0 = myproc();
    // Must acquire p->lock in order to
    // change p->state and then call sched.
    // Once we hold p->lock, we can be
    // guaranteed that we won't miss any wakeup
    // (wakeup locks p->lock),
    // so it's okay to release lk.
    if lk != &mut (*p).lock as *mut spinlock {
        //DOC: sleeplock0
        acquire(&mut (*p).lock); //DOC: sleeplock1
        release(lk);
    }
    // Go to sleep.
    (*p).chan = chan;
    (*p).state = SLEEPING;
    sched();
    // Tidy up.
    (*p).chan = ptr::null_mut();
    // Reacquire original lock.
    if lk != &mut (*p).lock as *mut spinlock {
        release(&mut (*p).lock);
        acquire(lk);
    };
}
// Wake up all processes sleeping on chan.
// Must be called without any p->lock.
#[no_mangle]
pub unsafe extern "C" fn wakeup(mut chan: *mut libc::c_void) {
    let mut p: *mut proc_0 = ptr::null_mut();
    p = proc.as_mut_ptr();
    while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
        acquire(&mut (*p).lock);
        if (*p).state as libc::c_uint == SLEEPING as libc::c_int as libc::c_uint
            && (*p).chan == chan
        {
            (*p).state = RUNNABLE
        }
        release(&mut (*p).lock);
        p = p.offset(1)
    }
}
// Wake up p if it is sleeping in wait(); used by exit().
// Caller must hold p->lock.
unsafe extern "C" fn wakeup1(mut p: *mut proc_0) {
    if holding(&mut (*p).lock) == 0 {
        panic(b"wakeup1\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    if (*p).chan == p as *mut libc::c_void
        && (*p).state as libc::c_uint == SLEEPING as libc::c_int as libc::c_uint
    {
        (*p).state = RUNNABLE
    };
}
// Kill the process with the given pid.
// The victim won't exit until it tries to return
// to user space (see usertrap() in trap.c).
#[no_mangle]
pub unsafe extern "C" fn kill(mut pid: libc::c_int) -> libc::c_int {
    let mut p: *mut proc_0 = ptr::null_mut();
    p = proc.as_mut_ptr();
    while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
        acquire(&mut (*p).lock);
        if (*p).pid == pid {
            (*p).killed = 1 as libc::c_int;
            if (*p).state as libc::c_uint == SLEEPING as libc::c_int as libc::c_uint {
                // Wake process from sleep().
                (*p).state = RUNNABLE
            }
            release(&mut (*p).lock);
            return 0 as libc::c_int;
        }
        release(&mut (*p).lock);
        p = p.offset(1)
    }
    -(1 as libc::c_int)
}
// Copy to either a user address, or kernel address,
// depending on usr_dst.
// Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn either_copyout(
    mut user_dst: libc::c_int,
    mut dst: uint64,
    mut src: *mut libc::c_void,
    mut len: uint64,
) -> libc::c_int {
    let mut p: *mut proc_0 = myproc();
    if user_dst != 0 {
        copyout((*p).pagetable, dst, src as *mut libc::c_char, len)
    } else {
        memmove(
            dst as *mut libc::c_char as *mut libc::c_void,
            src,
            len as uint,
        );
        0 as libc::c_int
    }
}
// Copy from either a user address, or kernel address,
// depending on usr_src.
// Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn either_copyin(
    mut dst: *mut libc::c_void,
    mut user_src: libc::c_int,
    mut src: uint64,
    mut len: uint64,
) -> libc::c_int {
    let mut p: *mut proc_0 = myproc();
    if user_src != 0 {
        copyin((*p).pagetable, dst as *mut libc::c_char, src, len)
    } else {
        memmove(
            dst,
            src as *mut libc::c_char as *const libc::c_void,
            len as uint,
        );
        0 as libc::c_int
    }
}
// Print a process listing to console.  For debugging.
// Runs when user types ^P on console.
// No lock to avoid wedging a stuck machine further.
#[no_mangle]
pub unsafe extern "C" fn procdump() {
    static mut states: [*mut libc::c_char; 5] = [
        b"unused\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        b"sleep \x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        b"runble\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        b"run   \x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        b"zombie\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    ];
    let mut p: *mut proc_0 = ptr::null_mut();
    let mut state: *mut libc::c_char = ptr::null_mut();
    printf(b"\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    p = proc.as_mut_ptr();
    while p < &mut *proc.as_mut_ptr().offset(NPROC as isize) as *mut proc_0 {
        if (*p).state as libc::c_uint != UNUSED as libc::c_int as libc::c_uint {
            if (*p).state as libc::c_uint >= 0 as libc::c_int as libc::c_uint
                && ((*p).state as libc::c_ulong)
                    < (::core::mem::size_of::<[*mut libc::c_char; 5]>() as libc::c_ulong)
                        .wrapping_div(::core::mem::size_of::<*mut libc::c_char>() as libc::c_ulong)
                && !states[(*p).state as usize].is_null()
            {
                state = states[(*p).state as usize]
            } else {
                state = b"???\x00" as *const u8 as *const libc::c_char as *mut libc::c_char
            }
            printf(
                b"%d %s %s\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
                (*p).pid,
                state,
                (*p).name.as_mut_ptr(),
            );
            printf(b"\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        }
        p = p.offset(1)
    }
}
