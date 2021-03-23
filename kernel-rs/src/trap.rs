use core::mem;

use crate::{
    kernel::{kernel, Kernel},
    memlayout::{TRAMPOLINE, TRAPFRAME, UART0_IRQ, VIRTIO0_IRQ},
    ok_or,
    plic::{plic_claim, plic_complete},
    println,
    proc::{cpuid, CurrentProc, Procstate},
    riscv::{
        intr_get, intr_off, intr_on, r_satp, r_scause, r_sepc, r_sip, r_stval, r_tp, w_sepc, w_sip,
        w_stvec, Sstatus, PGSIZE,
    },
};

extern "C" {
    // trampoline.S
    static mut trampoline: [u8; 0];

    static mut uservec: [u8; 0];

    static mut userret: [u8; 0];

    // In kernelvec.S, calls kerneltrap().
    fn kernelvec();
}

pub fn trapinit() {}

/// Set up to take exceptions and traps while in the kernel.
pub unsafe fn trapinithart() {
    unsafe { w_stvec(kernelvec as _) };
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
    unsafe { w_stvec(kernelvec as _) };

    // Safe since usertrap can be reached only after the initialization of the kernel
    let kernel = unsafe { kernel() };
    let mut proc = kernel.current_proc().expect("No current proc");

    // Save user program counter.
    proc.trap_frame_mut().epc = r_sepc();
    if r_scause() == 8 {
        // system call

        if proc.killed() {
            kernel.procs().exit_current(-1, &mut proc);
        }

        // sepc points to the ecall instruction,
        // but we want to return to the next instruction.
        proc.trap_frame_mut().epc = (proc.trap_frame().epc).wrapping_add(4);

        // An interrupt will change sstatus &c registers,
        // so don't enable until done with those registers.
        unsafe { intr_on() };
        proc.trap_frame_mut().a0 = ok_or!(
            kernel.syscall(proc.trap_frame_mut().a7 as i32, &mut proc),
            usize::MAX
        );
    } else {
        which_dev = unsafe { devintr(&kernel) };
        if which_dev == 0 {
            println!(
                "usertrap(): unexpected scause {:018p} pid={}",
                r_scause() as *const u8,
                proc.pid()
            );
            println!(
                "            sepc={:018p} stval={:018p}",
                r_sepc() as *const u8,
                r_stval() as *const u8
            );
            proc.kill();
        }
    }

    if proc.killed() {
        kernel.procs().exit_current(-1, &mut proc);
    }

    // Give up the CPU if this is a timer interrupt.
    if which_dev == 2 {
        unsafe { proc.proc_yield() };
    }

    unsafe { usertrapret(proc) };
}

/// Return to user space.
pub unsafe fn usertrapret(mut proc: CurrentProc<'_>) {
    // We're about to switch the destination of traps from
    // kerneltrap() to usertrap(), so turn off interrupts until
    // we're back in user space, where usertrap() is correct.
    unsafe { intr_off() };

    // Send syscalls, interrupts, and exceptions to trampoline.S.
    unsafe {
        w_stvec(
            TRAMPOLINE
                .wrapping_add(uservec.as_mut_ptr().offset_from(trampoline.as_mut_ptr()) as usize),
        )
    };

    // Set up trapframe values that uservec will need when
    // the process next re-enters the kernel.

    // kernel page table
    proc.trap_frame_mut().kernel_satp = r_satp();

    // process's kernel stack
    proc.trap_frame_mut().kernel_sp = proc.deref_mut_data().kstack + PGSIZE;
    proc.trap_frame_mut().kernel_trap = usertrap as usize;

    // hartid for cpuid()
    proc.trap_frame_mut().kernel_hartid = r_tp();

    // Set up the registers that trampoline.S's sret will use
    // to get to user space.

    // Set S Previous Privilege mode to User.
    let mut x = Sstatus::read();

    // Clear SPP to 0 for user mode.
    x.remove(Sstatus::SPP);

    // Enable interrupts in user mode.
    x.insert(Sstatus::SPIE);
    unsafe { x.write() };

    // Set S Exception Program Counter to the saved user pc.
    unsafe { w_sepc(proc.trap_frame().epc) };

    // Tell trampoline.S the user page table to switch to.
    let satp: usize = proc.memory().satp();

    // Jump to trampoline.S at the top of memory, which
    // switches to the user page table, restores user registers,
    // and switches to user mode with sret.
    let fn_0: usize =
        TRAMPOLINE + unsafe { userret.as_ptr().offset_from(trampoline.as_ptr()) } as usize;
    let fn_0 = unsafe { mem::transmute::<_, unsafe extern "C" fn(usize, usize) -> ()>(fn_0) };
    unsafe { fn_0(TRAPFRAME, satp) };
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

    // Safe since kerneltrap can be reached only after the initialization of the kernel
    let kernel = unsafe { kernel() };

    let which_dev = unsafe { devintr(&kernel) };
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
    if which_dev == 2 {
        if let Some(proc) = kernel.current_proc() {
            // Reading state without lock is safe because `proc_yield` and `sched`
            // is called after we check if current process is `RUNNING`.
            if unsafe { (*proc.info.get_mut_raw()).state } == Procstate::RUNNING {
                unsafe { proc.proc_yield() };
            }
        }
    }

    // The yield() may have caused some traps to occur,
    // so restore trap registers for use by kernelvec.S's sepc instruction.
    unsafe { w_sepc(sepc) };
    unsafe { sstatus.write() };
}

fn clockintr(kernel: &Kernel) {
    let mut ticks = kernel.ticks.lock();
    *ticks = ticks.wrapping_add(1);
    ticks.wakeup();
}

/// Check if it's an external interrupt or software interrupt,
/// and handle it.
/// Returns 2 if timer interrupt,
/// 1 if other device,
/// 0 if not recognized.
unsafe fn devintr(kernel: &Kernel) -> i32 {
    let scause: usize = r_scause();

    if scause & 0x8000000000000000 != 0 && scause & 0xff == 9 {
        // This is a supervisor external interrupt, via PLIC.

        // irq indicates which device interrupted.
        let irq = unsafe { plic_claim() };

        if irq as usize == UART0_IRQ {
            kernel.uart.intr();
        } else if irq as usize == VIRTIO0_IRQ {
            kernel.file_system.log.disk.lock().intr();
        } else if irq != 0 {
            println!("unexpected interrupt irq={}\n", irq);
        }

        // The PLIC allows each device to raise at most one
        // interrupt at a time; tell the PLIC the device is
        // now allowed to interrupt again.
        if irq != 0 {
            unsafe { plic_complete(irq) };
        }

        1
    } else if scause == 0x8000000000000001 {
        // Software interrupt from a machine-mode timer interrupt,
        // forwarded by timervec in kernelvec.S.

        if cpuid() == 0 {
            clockintr(kernel);
        }

        // Acknowledge the software interrupt by clearing
        // the SSIP bit in sip.
        unsafe { w_sip(r_sip() & !2) };

        2
    } else {
        0
    }
}
