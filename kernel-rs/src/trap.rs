use crate::libc;
use crate::{
    memlayout::{TRAMPOLINE, TRAPFRAME, UART0_IRQ, VIRTIO0_IRQ},
    plic::{plic_claim, plic_complete},
    printf::{panic, printf},
    proc::{cpuid, exit, myproc, proc_0, wakeup, yield_0, RUNNING},
    riscv::{
        intr_get, intr_off, intr_on, make_satp, r_satp, r_scause, r_sepc, r_sip, r_sstatus,
        r_stval, r_tp, w_sepc, w_sip, w_sstatus, w_stvec, PGSIZE, SSTATUS_SPIE, SSTATUS_SPP,
    },
    spinlock::Spinlock,
    syscall::syscall,
    uart::uartintr,
    virtio_disk::virtio_disk_intr,
};
use core::mem;

extern "C" {
    // trampoline.S
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

pub static mut tickslock: Spinlock = Spinlock::zeroed();
pub static mut ticks: u32 = 0;

pub unsafe fn trapinit() {
    tickslock.initlock(b"time\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
}

/// set up to take exceptions and traps while in the kernel.
pub unsafe fn trapinithart() {
    w_stvec(kernelvec as _);
}

/// handle an interrupt, exception, or system call from user space.
/// called from trampoline.S
#[no_mangle]
pub unsafe extern "C" fn usertrap() {
    let mut which_dev: i32 = 0 as i32;

    if r_sstatus() & SSTATUS_SPP as usize != 0usize {
        panic(
            b"usertrap: not from user mode\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }

    // send interrupts and exceptions to kerneltrap(),
    // since we're now in the kernel.
    w_stvec(kernelvec as _);

    let mut p: *mut proc_0 = myproc();

    // save user program counter.
    (*(*p).tf).epc = r_sepc();
    if r_scause() == 8 {
        // system call

        if (*p).killed != 0 {
            exit(-1);
        }

        // sepc points to the ecall instruction,
        // but we want to return to the next instruction.
        (*(*p).tf).epc = ((*(*p).tf).epc).wrapping_add(4 as usize);

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
        exit(-1);
    }

    // give up the CPU if this is a timer interrupt.
    if which_dev == 2 as i32 {
        yield_0();
    }

    usertrapret();
}

/// return to user space
pub unsafe fn usertrapret() {
    let mut p: *mut proc_0 = myproc();

    // turn off interrupts, since we're switching
    // now from kerneltrap() to usertrap().
    intr_off();

    // send syscalls, interrupts, and exceptions to trampoline.S
    w_stvec(
        (TRAMPOLINE
            + uservec
                .as_mut_ptr()
                .wrapping_offset_from(trampoline.as_mut_ptr()) as i64) as usize,
    );

    // set up trapframe values that uservec will need when
    // the process next re-enters the kernel.

    // kernel page table
    (*(*p).tf).kernel_satp = r_satp();

    // process's kernel stack
    (*(*p).tf).kernel_sp = (*p).kstack.wrapping_add(PGSIZE as usize);
    (*(*p).tf).kernel_trap = usertrap as usize;

    // hartid for cpuid()
    (*(*p).tf).kernel_hartid = r_tp();

    // set up the registers that trampoline.S's sret will use
    // to get to user space.

    // set S Previous Privilege mode to User.
    let mut x: usize = r_sstatus();

    // clear SPP to 0 for user mode
    x &= !SSTATUS_SPP as usize;

    // enable interrupts in user mode
    x |= SSTATUS_SPIE as usize;
    w_sstatus(x);

    // set S Exception Program Counter to the saved user pc.
    w_sepc((*(*p).tf).epc);

    // tell trampoline.S the user page table to switch to.
    let mut satp: usize = make_satp((*p).pagetable as usize);

    // jump to trampoline.S at the top of memory, which
    // switches to the user page table, restores user registers,
    // and switches to user mode with sret.
    let mut fn_0: usize = (TRAMPOLINE
        + userret
            .as_mut_ptr()
            .wrapping_offset_from(trampoline.as_mut_ptr()) as i64)
        as usize;
    let fn_0 = mem::transmute::<usize, unsafe extern "C" fn(_: usize, _: usize) -> ()>(fn_0);
    fn_0(TRAPFRAME as usize, satp);
}

/// interrupts and exceptions from kernel code go here via kernelvec,
/// on whatever the current kernel stack is.
/// must be 4-byte aligned to fit in stvec.
#[no_mangle]
pub unsafe fn kerneltrap() {
    let mut sepc: usize = r_sepc();
    let mut sstatus: usize = r_sstatus();
    let mut scause: usize = r_scause();

    if sstatus & SSTATUS_SPP as usize == 0 {
        panic(
            b"kerneltrap: not from supervisor mode\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }

    if intr_get() != 0 {
        panic(
            b"kerneltrap: interrupts enabled\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }

    let which_dev = devintr();
    if which_dev == 0 {
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
    if which_dev == 2 && !myproc().is_null() && (*myproc()).state == RUNNING {
        yield_0();
    }

    // the yield() may have caused some traps to occur,
    // so restore trap registers for use by kernelvec.S's sepc instruction.
    w_sepc(sepc);
    w_sstatus(sstatus);
}

pub unsafe fn clockintr() {
    tickslock.acquire();
    ticks = ticks.wrapping_add(1);
    wakeup(&mut ticks as *mut u32 as *mut libc::c_void);
    tickslock.release();
}

/// check if it's an external interrupt or software interrupt,
/// and handle it.
/// returns 2 if timer interrupt,
/// 1 if other device,
/// 0 if not recognized.
pub unsafe fn devintr() -> i32 {
    let mut scause: usize = r_scause();

    if scause & 0x8000000000000000 != 0 && scause & 0xff == 9 {
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
    } else if scause == 0x8000000000000001 {
        // software interrupt from a machine-mode timer interrupt,
        // forwarded by timervec in kernelvec.S.

        if cpuid() == 0 {
            clockintr();
        }

        // acknowledge the software interrupt by clearing
        // the SSIP bit in sip.
        w_sip(r_sip() & !2);

        2
    } else {
        0
    }
}
