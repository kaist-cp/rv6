use crate::{ libc, proc::{ proc_0, cpu }, spinlock::Spinlock };
extern "C" {
    // pub type inode;
    // pub type file;
    // printf.c
    #[no_mangle]
    fn printf(_: *mut libc::c_char, _: ...);
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    // proc.c
    #[no_mangle]
    fn cpuid() -> libc::c_int;
    #[no_mangle]
    fn exit(_: libc::c_int);
    #[no_mangle]
    fn myproc() -> *mut proc_0;
    #[no_mangle]
    fn wakeup(_: *mut libc::c_void);
    #[link_name = "yield"]
    fn yield_0();
    // spinlock.c
    #[no_mangle]
    fn acquire(_: *mut Spinlock);
    #[no_mangle]
    fn initlock(_: *mut Spinlock, _: *mut libc::c_char);
    #[no_mangle]
    fn release(_: *mut Spinlock);
    #[no_mangle]
    fn syscall();
    #[no_mangle]
    fn uartintr();
    #[no_mangle]
    fn plic_claim() -> libc::c_int;
    #[no_mangle]
    fn plic_complete(_: libc::c_int);
    #[no_mangle]
    fn virtio_disk_intr();
    #[no_mangle]
    static mut trampoline: [libc::c_char; 0];
    #[no_mangle]
    static mut uservec: [libc::c_char; 0];
    #[no_mangle]
    static mut userret: [libc::c_char; 0];
    // in kernelvec.S, calls kerneltrap().
    #[no_mangle]
    fn kernelvec();
}
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
pub type pagetable_t = *mut uint64;

pub type procstate = libc::c_uint;
pub const ZOMBIE: procstate = 4;
pub const RUNNING: procstate = 3;
pub const RUNNABLE: procstate = 2;
pub const SLEEPING: procstate = 1;
pub const UNUSED: procstate = 0;
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
pub const UART0_IRQ: libc::c_int = 10 as libc::c_int;
// virtio mmio interface
pub const VIRTIO0_IRQ: libc::c_int = 1 as libc::c_int;
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
pub const SSTATUS_SPP: libc::c_long = (1 as libc::c_long) << 8 as libc::c_int;
// Previous mode, 1=Supervisor, 0=User
pub const SSTATUS_SPIE: libc::c_long = (1 as libc::c_long) << 5 as libc::c_int;
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
// Supervisor Interrupt Pending
#[inline]
unsafe extern "C" fn r_sip() -> uint64 {
    let mut x: uint64 = 0;
    llvm_asm!("csrr $0, sip" : "=r" (x) : : : "volatile");
    x
}
#[inline]
unsafe extern "C" fn w_sip(mut x: uint64) {
    llvm_asm!("csrw sip, $0" : : "r" (x) : : "volatile");
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
// machine exception program counter, holds the
// instruction address to which a return from
// exception will go.
#[inline]
unsafe extern "C" fn w_sepc(mut x: uint64) {
    llvm_asm!("csrw sepc, $0" : : "r" (x) : : "volatile");
}
#[inline]
unsafe extern "C" fn r_sepc() -> uint64 {
    let mut x: uint64 = 0;
    llvm_asm!("csrr $0, sepc" : "=r" (x) : : : "volatile");
    x
}
// Supervisor Trap-Vector Base Address
// low two bits are mode.
#[inline]
unsafe extern "C" fn w_stvec(mut x: uint64) {
    llvm_asm!("csrw stvec, $0" : : "r" (x) : : "volatile");
}
// use riscv's sv39 page table scheme.
pub const SATP_SV39: libc::c_long = (8 as libc::c_long) << 60 as libc::c_int;
#[inline]
unsafe extern "C" fn r_satp() -> uint64 {
    let mut x: uint64 = 0;
    llvm_asm!("csrr $0, satp" : "=r" (x) : : : "volatile");
    x
}
// Supervisor Trap Cause
#[inline]
unsafe extern "C" fn r_scause() -> uint64 {
    let mut x: uint64 = 0;
    llvm_asm!("csrr $0, scause" : "=r" (x) : : : "volatile");
    x
}
// Supervisor Trap Value
#[inline]
unsafe extern "C" fn r_stval() -> uint64 {
    let mut x: uint64 = 0;
    llvm_asm!("csrr $0, stval" : "=r" (x) : : : "volatile");
    x
}
// enable device interrupts
#[inline]
unsafe extern "C" fn intr_on() {
    w_sie(
        r_sie() | SIE_SEIE as libc::c_ulong | SIE_STIE as libc::c_ulong | SIE_SSIE as libc::c_ulong,
    );
    w_sstatus(r_sstatus() | SSTATUS_SIE as libc::c_ulong);
}
// disable device interrupts
#[inline]
unsafe extern "C" fn intr_off() {
    w_sstatus(r_sstatus() & !SSTATUS_SIE as libc::c_ulong);
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
#[no_mangle]
pub static mut tickslock: Spinlock = Spinlock {
    locked: 0,
    name: 0 as *const libc::c_char as *mut libc::c_char,
    cpu: 0 as *const cpu as *mut cpu,
};
#[no_mangle]
pub static mut ticks: uint = 0;
// trap.c
#[no_mangle]
pub unsafe extern "C" fn trapinit() {
    initlock(
        &mut tickslock,
        b"time\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
}
// set up to take exceptions and traps while in the kernel.
#[no_mangle]
pub unsafe extern "C" fn trapinithart() {
    w_stvec(::core::mem::transmute::<
        Option<unsafe extern "C" fn() -> ()>,
        uint64,
    >(Some(::core::mem::transmute::<
        unsafe extern "C" fn() -> (),
        unsafe extern "C" fn() -> (),
    >(kernelvec))));
}
//
// handle an interrupt, exception, or system call from user space.
// called from trampoline.S
//
#[no_mangle]
pub unsafe extern "C" fn usertrap() {
    let mut which_dev: libc::c_int = 0 as libc::c_int;
    if r_sstatus() & SSTATUS_SPP as libc::c_ulong != 0 as libc::c_int as libc::c_ulong {
        panic(
            b"usertrap: not from user mode\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    // send interrupts and exceptions to kerneltrap(),
    // since we're now in the kernel.
    w_stvec(::core::mem::transmute::<
        Option<unsafe extern "C" fn() -> ()>,
        uint64,
    >(Some(::core::mem::transmute::<
        unsafe extern "C" fn() -> (),
        unsafe extern "C" fn() -> (),
    >(kernelvec))));
    let mut p: *mut proc_0 = myproc();
    // save user program counter.
    (*(*p).tf).epc = r_sepc();
    if r_scause() == 8 as libc::c_int as libc::c_ulong {
        // system call
        if (*p).killed != 0 {
            exit(-(1 as libc::c_int));
        }
        // sepc points to the ecall instruction,
        // but we want to return to the next instruction.
        (*(*p).tf).epc = ((*(*p).tf).epc as libc::c_ulong)
            .wrapping_add(4 as libc::c_int as libc::c_ulong) as uint64
            as uint64;
        // an interrupt will change sstatus &c registers,
        // so don't enable until done with those registers.
        intr_on();
        syscall();
    } else {
        which_dev = devintr();
        if which_dev == 0 as libc::c_int {
            printf(
                b"usertrap(): unexpected scause %p pid=%d\n\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
                r_scause(),
                (*p).pid,
            );
            printf(
                b"            sepc=%p stval=%p\n\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
                r_sepc(),
                r_stval(),
            );
            (*p).killed = 1 as libc::c_int
        }
    }
    if (*p).killed != 0 {
        exit(-(1 as libc::c_int));
    }
    // give up the CPU if this is a timer interrupt.
    if which_dev == 2 as libc::c_int {
        yield_0();
    }
    usertrapret();
}
//
// return to user space
//
#[no_mangle]
pub unsafe extern "C" fn usertrapret() {
    let mut p: *mut proc_0 = myproc();
    // turn off interrupts, since we're switching
    // now from kerneltrap() to usertrap().
    intr_off();
    // send syscalls, interrupts, and exceptions to trampoline.S
    w_stvec(
        (TRAMPOLINE
            + uservec
                .as_mut_ptr()
                .wrapping_offset_from(trampoline.as_mut_ptr()) as libc::c_long) as uint64,
    );
    // set up trapframe values that uservec will need when
    // the process next re-enters the kernel.
    (*(*p).tf).kernel_satp = r_satp(); // kernel page table
    (*(*p).tf).kernel_sp = (*p).kstack.wrapping_add(PGSIZE as libc::c_ulong); // process's kernel stack
    (*(*p).tf).kernel_trap = ::core::mem::transmute::<Option<unsafe extern "C" fn() -> ()>, uint64>(
        Some(usertrap as unsafe extern "C" fn() -> ()),
    ); // hartid for cpuid()
    (*(*p).tf).kernel_hartid = r_tp();
    // set up the registers that trampoline.S's sret will use
    // to get to user space.
    // set S Previous Privilege mode to User.
    let mut x: libc::c_ulong = r_sstatus(); // clear SPP to 0 for user mode
    x &= !SSTATUS_SPP as libc::c_ulong; // enable interrupts in user mode
    x |= SSTATUS_SPIE as libc::c_ulong;
    w_sstatus(x);
    // set S Exception Program Counter to the saved user pc.
    w_sepc((*(*p).tf).epc);
    // tell trampoline.S the user page table to switch to.
    let mut satp: uint64 =
        SATP_SV39 as libc::c_ulong | (*p).pagetable as uint64 >> 12 as libc::c_int;
    // jump to trampoline.S at the top of memory, which
    // switches to the user page table, restores user registers,
    // and switches to user mode with sret.
    let mut fn_0: uint64 = (TRAMPOLINE
        + userret
            .as_mut_ptr()
            .wrapping_offset_from(trampoline.as_mut_ptr()) as libc::c_long)
        as uint64;
    ::core::mem::transmute::<
        libc::intptr_t,
        Option<unsafe extern "C" fn(_: uint64, _: uint64) -> ()>,
    >(fn_0 as libc::intptr_t)
    .expect("non-null function pointer")(TRAPFRAME as uint64, satp);
}
// interrupts and exceptions from kernel code go here via kernelvec,
// on whatever the current kernel stack is.
// must be 4-byte aligned to fit in stvec.
#[no_mangle]
pub unsafe extern "C" fn kerneltrap() {
    let mut which_dev: libc::c_int = 0 as libc::c_int;
    let mut sepc: uint64 = r_sepc();
    let mut sstatus: uint64 = r_sstatus();
    let mut scause: uint64 = r_scause();
    if sstatus & SSTATUS_SPP as libc::c_ulong == 0 as libc::c_int as libc::c_ulong {
        panic(
            b"kerneltrap: not from supervisor mode\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    if intr_get() != 0 as libc::c_int {
        panic(
            b"kerneltrap: interrupts enabled\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    which_dev = devintr();
    if which_dev == 0 as libc::c_int {
        printf(
            b"scause %p\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
            scause,
        );
        printf(
            b"sepc=%p stval=%p\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
            r_sepc(),
            r_stval(),
        );
        panic(b"kerneltrap\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
    }
    // give up the CPU if this is a timer interrupt.
    if which_dev == 2 as libc::c_int
        && !myproc().is_null()
        && (*myproc()).state as libc::c_uint == RUNNING as libc::c_int as libc::c_uint
    {
        yield_0();
    }
    // the yield() may have caused some traps to occur,
    // so restore trap registers for use by kernelvec.S's sepc instruction.
    w_sepc(sepc);
    w_sstatus(sstatus);
}
#[no_mangle]
pub unsafe extern "C" fn clockintr() {
    acquire(&mut tickslock);
    ticks = ticks.wrapping_add(1);
    wakeup(&mut ticks as *mut uint as *mut libc::c_void);
    release(&mut tickslock);
}
// check if it's an external interrupt or software interrupt,
// and handle it.
// returns 2 if timer interrupt,
// 1 if other device,
// 0 if not recognized.
#[no_mangle]
pub unsafe extern "C" fn devintr() -> libc::c_int {
    let mut scause: uint64 = r_scause();
    if scause & 0x8000000000000000 as libc::c_ulong != 0
        && scause & 0xff as libc::c_int as libc::c_ulong == 9 as libc::c_int as libc::c_ulong
    {
        // this is a supervisor external interrupt, via PLIC.
        // irq indicates which device interrupted.
        let mut irq: libc::c_int = plic_claim();
        if irq == UART0_IRQ {
            uartintr();
        } else if irq == VIRTIO0_IRQ {
            virtio_disk_intr();
        }
        plic_complete(irq);
        1 as libc::c_int
    } else if scause == 0x8000000000000001 as libc::c_ulong {
        // software interrupt from a machine-mode timer interrupt,
        // forwarded by timervec in kernelvec.S.
        if cpuid() == 0 as libc::c_int {
            clockintr();
        }
        // acknowledge the software interrupt by clearing
        // the SSIP bit in sip.
        w_sip(r_sip() & !(2 as libc::c_int) as libc::c_ulong);
        2 as libc::c_int
    } else {
        0 as libc::c_int
    }
}
