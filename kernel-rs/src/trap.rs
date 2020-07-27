use crate::{
    libc,
    proc::{cpu, myproc, proc_0},
    spinlock::{acquire, initlock, release, Spinlock},
};
extern "C" {
    // printf.c
    #[no_mangle]
    fn printf(_: *mut libc::c_char, _: ...);
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    // proc.c
    #[no_mangle]
    fn cpuid() -> i32;
    #[no_mangle]
    fn exit(_: i32);
    #[no_mangle]
    fn wakeup(_: *mut libc::c_void);
    #[link_name = "yield"]
    fn yield_0();
    #[no_mangle]
    fn syscall();
    #[no_mangle]
    fn uartintr();
    #[no_mangle]
    fn plic_claim() -> i32;
    #[no_mangle]
    fn plic_complete(_: i32);
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
pub type pagetable_t = *mut u64;
pub type procstate = u32;
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
pub const UART0_IRQ: i32 = 10;
// virtio mmio interface
pub const VIRTIO0_IRQ: i32 = 1;
// local interrupt controller, which contains the timer.
// cycles since boot.
// qemu puts programmable interrupt controller here.
// the kernel expects there to be RAM
// for use by the kernel and user pages
// from physical address 0x80000000 to PHYSTOP.
// map the trampoline page to the highest address,
// in both user and kernel space.
pub const TRAMPOLINE: i64 = MAXVA - PGSIZE as i64;
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
pub const TRAPFRAME: i64 = TRAMPOLINE - PGSIZE as i64;
// Supervisor Status Register, sstatus
pub const SSTATUS_SPP: i64 = (1 as i64) << 8 as i32;
// Previous mode, 1=Supervisor, 0=User
pub const SSTATUS_SPIE: i64 = (1 as i64) << 5 as i32;
// Supervisor Previous Interrupt Enable
// User Previous Interrupt Enable
pub const SSTATUS_SIE: i64 = (1 as i64) << 1 as i32;
/// Supervisor Interrupt Enable
/// User Interrupt Enable
#[inline]
unsafe extern "C" fn r_sstatus() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, sstatus" : "=r" (x) : : : "volatile");
    x
}
#[inline]
unsafe extern "C" fn w_sstatus(mut x: u64) {
    llvm_asm!("csrw sstatus, $0" : : "r" (x) : : "volatile");
}
/// Supervisor Interrupt Pending
#[inline]
unsafe extern "C" fn r_sip() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, sip" : "=r" (x) : : : "volatile");
    x
}
#[inline]
unsafe extern "C" fn w_sip(mut x: u64) {
    llvm_asm!("csrw sip, $0" : : "r" (x) : : "volatile");
}
// Supervisor Interrupt Enable
pub const SIE_SEIE: i64 = (1 as i64) << 9 as i32;
// external
pub const SIE_STIE: i64 = (1 as i64) << 5 as i32;
// timer
pub const SIE_SSIE: i64 = (1 as i64) << 1 as i32;
// software
#[inline]
unsafe extern "C" fn r_sie() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, sie" : "=r" (x) : : : "volatile");
    x
}
#[inline]
unsafe extern "C" fn w_sie(mut x: u64) {
    llvm_asm!("csrw sie, $0" : : "r" (x) : : "volatile");
}
/// machine exception program counter, holds the
/// instruction address to which a return from
/// exception will go.
#[inline]
unsafe extern "C" fn w_sepc(mut x: u64) {
    llvm_asm!("csrw sepc, $0" : : "r" (x) : : "volatile");
}
#[inline]
unsafe extern "C" fn r_sepc() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, sepc" : "=r" (x) : : : "volatile");
    x
}
/// Supervisor Trap-Vector Base Address
/// low two bits are mode.
#[inline]
unsafe extern "C" fn w_stvec(mut x: u64) {
    llvm_asm!("csrw stvec, $0" : : "r" (x) : : "volatile");
}
// use riscv's sv39 page table scheme.
pub const SATP_SV39: i64 = (8 as i64) << 60 as i32;
#[inline]
unsafe extern "C" fn r_satp() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, satp" : "=r" (x) : : : "volatile");
    x
}
/// Supervisor Trap Cause
#[inline]
unsafe extern "C" fn r_scause() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, scause" : "=r" (x) : : : "volatile");
    x
}
/// Supervisor Trap Value
#[inline]
unsafe extern "C" fn r_stval() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("csrr $0, stval" : "=r" (x) : : : "volatile");
    x
}
/// enable device interrupts
#[inline]
unsafe extern "C" fn intr_on() {
    w_sie(r_sie() | SIE_SEIE as u64 | SIE_STIE as u64 | SIE_SSIE as u64);
    w_sstatus(r_sstatus() | SSTATUS_SIE as u64);
}
/// disable device interrupts
#[inline]
unsafe extern "C" fn intr_off() {
    w_sstatus(r_sstatus() & !SSTATUS_SIE as u64);
}
/// are device interrupts enabled?
#[inline]
unsafe extern "C" fn intr_get() -> i32 {
    let mut x: u64 = r_sstatus();
    (x & SSTATUS_SIE as u64 != 0 as i32 as u64) as i32
}
/// read and write tp, the thread pointer, which holds
/// this core's hartid (core number), the index into cpus[].
#[inline]
unsafe extern "C" fn r_tp() -> u64 {
    let mut x: u64 = 0;
    llvm_asm!("mv $0, tp" : "=r" (x) : : : "volatile");
    x
}
pub const PGSIZE: i32 = 4096 as i32;
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
pub const MAXVA: i64 = (1 as i64) << (9 + 9 + 9 + 12 - 1) as i32;
#[no_mangle]
pub static mut tickslock: Spinlock = Spinlock {
    locked: 0,
    name: 0 as *const libc::c_char as *mut libc::c_char,
    cpu: 0 as *const cpu as *mut cpu,
};
#[no_mangle]
pub static mut ticks: u32 = 0;
// trap.c
#[no_mangle]
pub unsafe extern "C" fn trapinit() {
    initlock(
        &mut tickslock,
        b"time\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
}
/// set up to take exceptions and traps while in the kernel.
#[no_mangle]
pub unsafe extern "C" fn trapinithart() {
    w_stvec(::core::mem::transmute::<
        Option<unsafe extern "C" fn() -> ()>,
        u64,
    >(Some(::core::mem::transmute::<
        unsafe extern "C" fn() -> (),
        unsafe extern "C" fn() -> (),
    >(kernelvec))));
}

/// handle an interrupt, exception, or system call from user space.
/// called from trampoline.S
#[no_mangle]
pub unsafe extern "C" fn usertrap() {
    let mut which_dev: i32 = 0 as i32;
    if r_sstatus() & SSTATUS_SPP as u64 != 0 as i32 as u64 {
        panic(
            b"usertrap: not from user mode\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    // send interrupts and exceptions to kerneltrap(),
    // since we're now in the kernel.
    w_stvec(::core::mem::transmute::<
        Option<unsafe extern "C" fn() -> ()>,
        u64,
    >(Some(::core::mem::transmute::<
        unsafe extern "C" fn() -> (),
        unsafe extern "C" fn() -> (),
    >(kernelvec))));
    let mut p: *mut proc_0 = myproc();
    // save user program counter.
    (*(*p).tf).epc = r_sepc();
    if r_scause() == 8 as u64 {
        // system call
        if (*p).killed != 0 {
            exit(-(1 as i32));
        }
        // sepc points to the ecall instruction,
        // but we want to return to the next instruction.
        (*(*p).tf).epc = ((*(*p).tf).epc as u64).wrapping_add(4 as u64) as u64;
        // an interrupt will change sstatus &c registers,
        // so don't enable until done with those registers.
        intr_on();
        syscall();
    } else {
        which_dev = devintr();
        if which_dev == 0 as i32 {
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
            (*p).killed = 1 as i32
        }
    }
    if (*p).killed != 0 {
        exit(-(1 as i32));
    }
    // give up the CPU if this is a timer interrupt.
    if which_dev == 2 as i32 {
        yield_0();
    }
    usertrapret();
}

/// return to user space
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
                .wrapping_offset_from(trampoline.as_mut_ptr()) as i64) as u64,
    );
    // set up trapframe values that uservec will need when
    // the process next re-enters the kernel.
    (*(*p).tf).kernel_satp = r_satp(); // kernel page table
    (*(*p).tf).kernel_sp = (*p).kstack.wrapping_add(PGSIZE as u64); // process's kernel stack
    (*(*p).tf).kernel_trap = ::core::mem::transmute::<Option<unsafe extern "C" fn() -> ()>, u64>(
        Some(usertrap as unsafe extern "C" fn() -> ()),
    ); // hartid for cpuid()
    (*(*p).tf).kernel_hartid = r_tp();
    // set up the registers that trampoline.S's sret will use
    // to get to user space.
    // set S Previous Privilege mode to User.
    let mut x: u64 = r_sstatus(); // clear SPP to 0 for user mode
    x &= !SSTATUS_SPP as u64; // enable interrupts in user mode
    x |= SSTATUS_SPIE as u64;
    w_sstatus(x);
    // set S Exception Program Counter to the saved user pc.
    w_sepc((*(*p).tf).epc);
    // tell trampoline.S the user page table to switch to.
    let mut satp: u64 = SATP_SV39 as u64 | (*p).pagetable as u64 >> 12 as i32;
    // jump to trampoline.S at the top of memory, which
    // switches to the user page table, restores user registers,
    // and switches to user mode with sret.
    let mut fn_0: u64 = (TRAMPOLINE
        + userret
            .as_mut_ptr()
            .wrapping_offset_from(trampoline.as_mut_ptr()) as i64) as u64;
    ::core::mem::transmute::<isize, Option<unsafe extern "C" fn(_: u64, _: u64) -> ()>>(
        fn_0 as isize,
    )
    .expect("non-null function pointer")(TRAPFRAME as u64, satp);
}
/// interrupts and exceptions from kernel code go here via kernelvec,
/// on whatever the current kernel stack is.
/// must be 4-byte aligned to fit in stvec.
#[no_mangle]
pub unsafe extern "C" fn kerneltrap() {
    let mut which_dev: i32 = 0 as i32;
    let mut sepc: u64 = r_sepc();
    let mut sstatus: u64 = r_sstatus();
    let mut scause: u64 = r_scause();
    if sstatus & SSTATUS_SPP as u64 == 0 as u64 {
        panic(
            b"kerneltrap: not from supervisor mode\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    if intr_get() != 0 as i32 {
        panic(
            b"kerneltrap: interrupts enabled\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    which_dev = devintr();
    if which_dev == 0 as i32 {
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
    if which_dev == 2 && !myproc().is_null() && (*myproc()).state as u32 == RUNNING as u32 {
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
    wakeup(&mut ticks as *mut u32 as *mut libc::c_void);
    release(&mut tickslock);
}
/// check if it's an external interrupt or software interrupt,
/// and handle it.
/// returns 2 if timer interrupt,
/// 1 if other device,
/// 0 if not recognized.
#[no_mangle]
pub unsafe extern "C" fn devintr() -> i32 {
    let mut scause: u64 = r_scause();
    if scause & 0x8000000000000000 as u64 != 0 && scause & 0xff as u64 == 9 as u64 {
        // this is a supervisor external interrupt, via PLIC.
        // irq indicates which device interrupted.
        let mut irq: i32 = plic_claim();
        if irq == UART0_IRQ {
            uartintr();
        } else if irq == VIRTIO0_IRQ {
            virtio_disk_intr();
        }
        plic_complete(irq);
        1
    } else if scause == 0x8000000000000001 as u64 {
        // software interrupt from a machine-mode timer interrupt,
        // forwarded by timervec in kernelvec.S.
        if cpuid() == 0 {
            clockintr();
        }
        // acknowledge the software interrupt by clearing
        // the SSIP bit in sip.
        w_sip(r_sip() & !(2 as i32) as u64);
        2
    } else {
        0
    }
}
