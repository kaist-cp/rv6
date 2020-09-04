use crate::libc;
use crate::{
    memlayout::{TRAMPOLINE, TRAPFRAME, UART0_IRQ, VIRTIO0_IRQ},
    plic::{plic_claim, plic_complete},
    println,
    proc::{cpuid, exit, myproc, proc_yield, wakeup, Proc, Procstate},
    riscv::{
        intr_get, intr_off, intr_on, make_satp, r_satp, r_scause, r_sepc, r_sip, Sstatus,
        r_stval, r_tp, w_sepc, w_sip, w_stvec, PGSIZE, 
    },
    spinlock::RawSpinlock,
    syscall::syscall,
    uart::Uart,
    virtio_disk::virtio_disk_intr,
};
use core::mem;

extern "C" {
    // trampoline.S
    #[no_mangle]
    static mut trampoline: [u8; 0];

    #[no_mangle]
    static mut uservec: [u8; 0];

    #[no_mangle]
    static mut userret: [u8; 0];

    // in kernelvec.S, calls kerneltrap().
    #[no_mangle]
    fn kernelvec();
}

pub static mut TICKSLOCK: RawSpinlock = RawSpinlock::zeroed();
pub static mut TICKS: u32 = 0;

pub unsafe fn trapinit() {
    TICKSLOCK.initlock("time");
}

/// set up to take exceptions and traps while in the kernel.
pub unsafe fn trapinithart() {
    w_stvec(kernelvec as _);
}

/// handle an interrupt, exception, or system call from user space.
/// called from trampoline.S
#[no_mangle]
pub unsafe extern "C" fn usertrap() {
    let mut which_dev: i32 = 0;

    if Sstatus::r_sstatus() & Sstatus::SPP != Sstatus::ZERO {
        panic!("usertrap: not from user mode");
    }

    // send interrupts and exceptions to kerneltrap(),
    // since we're now in the kernel.
    w_stvec(kernelvec as _);

    let mut p: *mut Proc = myproc();

    // save user program counter.
    (*(*p).tf).epc = r_sepc();
    if r_scause() == 8 {
        // system call

        if (*p).killed != 0 {
            exit(-1);
        }

        // sepc points to the ecall instruction,
        // but we want to return to the next instruction.
        (*(*p).tf).epc = ((*(*p).tf).epc).wrapping_add(4);

        // an interrupt will change sstatus &c registers,
        // so don't enable until done with those registers.
        intr_on();
        syscall();
    } else {
        which_dev = devintr();
        if which_dev == 0 {
            println!(
                "usertrap(): unexpected scause {:018p} pid={}",
                r_scause() as *const u8,
                (*p).pid
            );
            println!(
                "            sepc={:018p} stval={:018p}",
                r_sepc() as *const u8,
                r_stval() as *const u8
            );
            (*p).killed = 1;
        }
    }

    if (*p).killed != 0 {
        exit(-1);
    }

    // give up the CPU if this is a timer interrupt.
    if which_dev == 2 {
        proc_yield();
    }

    usertrapret();
}

/// return to user space
pub unsafe fn usertrapret() {
    let mut p: *mut Proc = myproc();

    // turn off interrupts, since we're switching
    // now from kerneltrap() to usertrap().
    intr_off();

    // send syscalls, interrupts, and exceptions to trampoline.S
    w_stvec(
        TRAMPOLINE.wrapping_add(uservec.as_mut_ptr().offset_from(trampoline.as_mut_ptr()) as usize),
    );

    // set up trapframe values that uservec will need when
    // the process next re-enters the kernel.

    // kernel page table
    (*(*p).tf).kernel_satp = r_satp();

    // process's kernel stack
    (*(*p).tf).kernel_sp = (*p).kstack.wrapping_add(PGSIZE);
    (*(*p).tf).kernel_trap = usertrap as usize;

    // hartid for cpuid()
    (*(*p).tf).kernel_hartid = r_tp();

    // set up the registers that trampoline.S's sret will use
    // to get to user space.

    // set S Previous Privilege mode to User.
    let mut x = Sstatus::r_sstatus();

    // clear SPP to 0 for user mode
    x &= !Sstatus::SPP;

    // enable interrupts in user mode
    x |= Sstatus::SPIE;
    x.w_sstatus();

    // set S Exception Program Counter to the saved user pc.
    w_sepc((*(*p).tf).epc);

    // tell trampoline.S the user page table to switch to.
    let satp: usize = make_satp((*p).pagetable as usize);

    // jump to trampoline.S at the top of memory, which
    // switches to the user page table, restores user registers,
    // and switches to user mode with sret.
    let fn_0: usize =
        TRAMPOLINE.wrapping_add(userret.as_mut_ptr().offset_from(trampoline.as_mut_ptr()) as usize);
    let fn_0 = mem::transmute::<usize, unsafe extern "C" fn(_: usize, _: usize) -> ()>(fn_0);
    fn_0(TRAPFRAME, satp);
}

/// interrupts and exceptions from kernel code go here via kernelvec,
/// on whatever the current kernel stack is.
/// must be 4-byte aligned to fit in stvec.
#[no_mangle]
pub unsafe fn kerneltrap() {
    let sepc: usize = r_sepc();
    let sstatus = Sstatus::r_sstatus();
    let scause: usize = r_scause();

    if sstatus & Sstatus::SPP == Sstatus::ZERO {
        panic!("kerneltrap: not from supervisor mode");
    }

    if intr_get() {
        panic!("kerneltrap: interrupts enabled");
    }

    let which_dev = devintr();
    if which_dev == 0 {
        println!("scause {:018p}", scause as *const u8);
        println!(
            "sepc={:018p} stval={:018p}",
            r_sepc() as *const u8,
            r_stval() as *const u8
        );
        panic!("kerneltrap");
    }

    // give up the CPU if this is a timer interrupt.
    if which_dev == 2 && !myproc().is_null() && (*myproc()).state == Procstate::RUNNING {
        proc_yield();
    }

    // the yield() may have caused some traps to occur,
    // so restore trap registers for use by kernelvec.S's sepc instruction.
    w_sepc(sepc);
    sstatus.w_sstatus();
}

pub unsafe fn clockintr() {
    TICKSLOCK.acquire();
    TICKS = TICKS.wrapping_add(1);
    wakeup(&mut TICKS as *mut u32 as *mut libc::CVoid);
    TICKSLOCK.release();
}

/// check if it's an external interrupt or software interrupt,
/// and handle it.
/// returns 2 if timer interrupt,
/// 1 if other device,
/// 0 if not recognized.
pub unsafe fn devintr() -> i32 {
    let scause: usize = r_scause();

    if scause & 0x8000000000000000 != 0 && scause & 0xff == 9 {
        // this is a supervisor external interrupt, via .

        // irq indicates which device interrupted.
        let irq: usize = plic_claim();

        if irq == UART0_IRQ {
            Uart::intr();
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
