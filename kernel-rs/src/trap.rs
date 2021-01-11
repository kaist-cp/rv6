use crate::{
    fs::DISK,
    kernel::kernel,
    memlayout::{TRAMPOLINE, TRAPFRAME, UART0_IRQ, VIRTIO0_IRQ},
    plic::{plic_claim, plic_complete},
    println,
    proc::{cpuid, myproc, proc_yield, Proc, Procstate},
    riscv::{
        intr_get, intr_off, intr_on, make_satp, r_satp, r_scause, r_sepc, r_sip, r_stval, r_tp,
        w_sepc, w_sip, w_stvec, Sstatus, PGSIZE,
    },
};
use core::mem;

extern "C" {
    // trampoline.S
    static mut trampoline: [u8; 0];

    static mut uservec: [u8; 0];

    static mut userret: [u8; 0];

    // In kernelvec.S, calls kerneltrap().
    fn kernelvec();
}

pub unsafe fn trapinit() {}

/// Set up to take exceptions and traps while in the kernel.
pub unsafe fn trapinithart() {
    w_stvec(kernelvec as _);
}

/// Handle an interrupt, exception, or system call from user space.
/// Called from trampoline.S.
#[no_mangle]
pub unsafe extern "C" fn usertrap() {
    let mut which_dev: i32 = 0;

    assert!(
        !Sstatus::read().contains(Sstatus::SPP),
        "usertrap: not from user mode"
    );

    // Send interrupts and exceptions to kerneltrap(),
    // since we're now in the kernel.
    w_stvec(kernelvec as _);

    let p: *mut Proc = myproc();
    let mut data = &mut *(*p).data.get();

    // Save user program counter.
    (*data.trapframe).epc = r_sepc();
    if r_scause() == 8 {
        // system call

        if (*p).killed() {
            kernel().procs.exit_current(-1);
        }

        // sepc points to the ecall instruction,
        // but we want to return to the next instruction.
        (*data.trapframe).epc = ((*data.trapframe).epc).wrapping_add(4);

        // An interrupt will change sstatus &c registers,
        // so don't enable until done with those registers.
        intr_on();
        kernel().syscall();
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

    // Give up the CPU if this is a timer interrupt.
    if which_dev == 2 {
        proc_yield();
    }

    usertrapret();
}

/// Return to user space.
pub unsafe fn usertrapret() {
    let p: *mut Proc = myproc();
    let mut data = &mut *(*p).data.get();

    // We're about to switch the destination of traps from
    // kerneltrap() to usertrap(), so turn off interrupts until
    // we're back in user space, where usertrap() is correct.
    intr_off();

    // Send syscalls, interrupts, and exceptions to trampoline.S.
    w_stvec(
        TRAMPOLINE.wrapping_add(uservec.as_mut_ptr().offset_from(trampoline.as_mut_ptr()) as usize),
    );

    // Set up trapframe values that uservec will need when
    // the process next re-enters the kernel.

    // kernel page table
    (*data.trapframe).kernel_satp = r_satp();

    // process's kernel stack
    (*data.trapframe).kernel_sp = data.kstack.wrapping_add(PGSIZE);
    (*data.trapframe).kernel_trap = usertrap as usize;

    // hartid for cpuid()
    (*data.trapframe).kernel_hartid = r_tp();

    // Set up the registers that trampoline.S's sret will use
    // to get to user space.

    // Set S Previous Privilege mode to User.
    let mut x = Sstatus::read();

    // Clear SPP to 0 for user mode.
    x.remove(Sstatus::SPP);

    // Enable interrupts in user mode.
    x.insert(Sstatus::SPIE);
    x.write();

    // Set S Exception Program Counter to the saved user pc.
    w_sepc((*data.trapframe).epc);

    // Tell trampoline.S the user page table to switch to.
    let satp: usize = make_satp(data.pagetable.as_raw() as usize);

    // Jump to trampoline.S at the top of memory, which
    // switches to the user page table, restores user registers,
    // and switches to user mode with sret.
    let fn_0: usize =
        TRAMPOLINE.wrapping_add(userret.as_mut_ptr().offset_from(trampoline.as_mut_ptr()) as usize);
    let fn_0 = mem::transmute::<usize, unsafe extern "C" fn(_: usize, _: usize) -> ()>(fn_0);
    fn_0(TRAPFRAME, satp);
}

/// Interrupts and exceptions from kernel code go here via kernelvec,
/// on whatever the current kernel stack is.
#[no_mangle]
pub unsafe fn kerneltrap() {
    let sepc = r_sepc();
    let sstatus = Sstatus::read();
    let scause = r_scause();

    assert!(
        sstatus.contains(Sstatus::SPP),
        "kerneltrap: not from supervisor mode"
    );
    assert!(!intr_get(), "kerneltrap: interrupts enabled");

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

    // Give up the CPU if this is a timer interrupt.
    if which_dev == 2 && !myproc().is_null() && (*myproc()).state() == Procstate::RUNNING {
        proc_yield();
    }

    // The yield() may have caused some traps to occur,
    // so restore trap registers for use by kernelvec.S's sepc instruction.
    w_sepc(sepc);
    sstatus.write();
}

pub unsafe fn clockintr() {
    let mut ticks = kernel().ticks.lock();
    *ticks = ticks.wrapping_add(1);
    ticks.wakeup();
}

/// Check if it's an external interrupt or software interrupt,
/// and handle it.
/// Returns 2 if timer interrupt,
/// 1 if other device,
/// 0 if not recognized.
pub unsafe fn devintr() -> i32 {
    let scause: usize = r_scause();

    if scause & 0x8000000000000000 != 0 && scause & 0xff == 9 {
        // This is a supervisor external interrupt, via PLIC.

        // irq indicates which device interrupted.
        let irq = plic_claim();

        if irq as usize == UART0_IRQ {
            kernel().uart.intr();
        } else if irq as usize == VIRTIO0_IRQ {
            DISK.lock().virtio_intr();
        } else if irq != 0 {
            println!("unexpected interrupt irq={}\n", irq);
        }

        // The PLIC allows each device to raise at most one
        // interrupt at a time; tell the PLIC the device is
        // now allowed to interrupt again.
        if irq != 0 {
            plic_complete(irq);
        }

        1
    } else if scause == 0x8000000000000001 {
        // Software interrupt from a machine-mode timer interrupt,
        // forwarded by timervec in kernelvec.S.

        if cpuid() == 0 {
            clockintr();
        }

        // Acknowledge the software interrupt by clearing
        // the SSIP bit in sip.
        w_sip(r_sip() & !2);

        2
    } else {
        0
    }
}
