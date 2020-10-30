use crate::{
    kernel::kernel,
    memlayout::{TRAMPOLINE, TRAPFRAME, UART0_IRQ, VIRTIO0_IRQ},
    plic::{plic_claim, plic_complete},
    println,
    proc::{cpuid, myproc, proc_yield, Proc, Procstate},
    riscv::{
        intr_get, intr_off, intr_on, make_satp, r_satp, r_scause, r_sepc, r_sip, r_stval, r_tp,
        w_sepc, w_sip, w_stvec, Sstatus, PGSIZE,
    },
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

pub unsafe fn trapinit() {}

/// set up to take exceptions and traps while in the kernel.
pub unsafe fn trapinithart() {
    w_stvec(kernelvec as _);
}

/// handle an interrupt, exception, or system call from user space.
/// called from trampoline.S
#[no_mangle]
pub unsafe extern "C" fn usertrap() {
    let mut which_dev: i32 = 0;

    if Sstatus::read().contains(Sstatus::SPP) {
        panic!("usertrap: not from user mode");
    }

    // send interrupts and exceptions to kerneltrap(),
    // since we're now in the kernel.
    w_stvec(kernelvec as _);

    let p: *mut Proc = myproc();
    let mut data = &mut *(*p).data.get();

    // save user program counter.
    (*data.tf).epc = r_sepc();
    if r_scause() == 8 {
        // system call

        if (*p).killed() {
            kernel().procs.exit_current(-1);
        }

        // sepc points to the ecall instruction,
        // but we want to return to the next instruction.
        (*data.tf).epc = ((*data.tf).epc).wrapping_add(4);

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
                (*p).pid()
            );
            println!(
                "            sepc={:018p} stval={:018p}",
                r_sepc() as *const u8,
                r_stval() as *const u8
            );
            (*p).kill();
        }
    }

    if (*p).killed() {
        kernel().procs.exit_current(-1);
    }

    // give up the CPU if this is a timer interrupt.
    if which_dev == 2 {
        proc_yield();
    }

    usertrapret();
}

/// return to user space
pub unsafe fn usertrapret() {
    let p: *mut Proc = myproc();
    let mut data = &mut *(*p).data.get();

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
    (*data.tf).kernel_satp = r_satp();

    // process's kernel stack
    (*data.tf).kernel_sp = data.kstack.wrapping_add(PGSIZE);
    (*data.tf).kernel_trap = usertrap as usize;

    // hartid for cpuid()
    (*data.tf).kernel_hartid = r_tp();

    // set up the registers that trampoline.S's sret will use
    // to get to user space.

    // set S Previous Privilege mode to User.
    let mut x = Sstatus::read();

    // clear SPP to 0 for user mode
    x.remove(Sstatus::SPP);

    // enable interrupts in user mode
    x.insert(Sstatus::SPIE);
    x.write();

    // set S Exception Program Counter to the saved user pc.
    w_sepc((*data.tf).epc);

    // tell trampoline.S the user page table to switch to.
    let satp: usize = make_satp(data.pagetable.assume_init_mut().as_raw() as usize);

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
    let sepc = r_sepc();
    let sstatus = Sstatus::read();
    let scause = r_scause();

    if !sstatus.contains(Sstatus::SPP) {
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
    if which_dev == 2 && !myproc().is_null() && (*myproc()).state() == Procstate::RUNNING {
        proc_yield();
    }

    // the yield() may have caused some traps to occur,
    // so restore trap registers for use by kernelvec.S's sepc instruction.
    w_sepc(sepc);
    sstatus.write();
}

pub unsafe fn clockintr() {
    let mut ticks = kernel().ticks.lock();
    *ticks = ticks.wrapping_add(1);
    ticks.wakeup();
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
