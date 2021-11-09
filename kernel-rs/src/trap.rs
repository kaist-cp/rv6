use core::fmt;

use crate::{
    arch::interface::{ProcManager, TrapFrameManager, TrapManager},
    arch::TargetArch,
    hal::hal,
    kernel::{kernel_ref, KernelRef},
    ok_or,
    proc::{kernel_ctx, KernelCtx, Procstate},
};

/// In ARM.v8 architecture, interrupts are part
/// of a more general term: exceptions.
// enum ExceptionTypes {
//     SyncException,
//     IRQ,
//     FIQ,
//     SError,
// }

pub enum TrapTypes {
    Irq(IrqTypes),
    Syscall,
    BadTrap,
    TimerInterrupt,
}

#[derive(Debug)]
pub enum IrqTypes {
    Virtio,
    Uart,
    Others(IrqNum),
    Unknown(IrqNum),
}

pub type IrqNum = usize;

/// Handle an interrupt, exception, or system call from user space.
/// Called from trampoline.S.
#[no_mangle]
pub unsafe extern "C" fn usertrap(arg: usize) {
    // SAFETY
    // * usertrap can be reached only after the initialization of the kernel.
    // * It's the beginning of this thread, so there's no exsiting `KernelCtx` or `CurrentProc`.
    unsafe { kernel_ctx(|ctx| ctx.user_trap(arg)) };
}

/// Interrupts and exceptions from kernel code go here via kernelvec,
/// on whatever the current kernel stack is.
#[no_mangle]
pub unsafe fn kerneltrap(arg: usize) {
    // SAFETY: kerneltrap can be reached only after the initialization of the kernel.
    unsafe { kernel_ref(|kref| kref.kernel_trap(arg)) };
}

impl KernelCtx<'_, '_> {
    /// `user_trap` can be reached only from the user mode, so it is a method of `KernelCtx`.
    unsafe fn user_trap(mut self, arg: usize) -> ! {
        assert!(
            TargetArch::is_user_trap(),
            "usertrap: not from user mode(EL0)"
        );

        // Send interrupts and exceptions to kerneltrap(),
        // since we're now in the kernel.
        // SAFETY: We are in a kerel mode now.
        unsafe {
            TargetArch::switch_to_kernel_vec();
        }

        let mut guard = self.proc().lock();
        let info = guard.deref_mut_info();

        // Save user program counter.
        unsafe {
            (*info.trap_frame).set_pc(TargetArch::r_epc());
        }

        let trap_type = TargetArch::get_trap_type(arg);

        // SAFETY: Actually received trap with type of `trap_type`.
        unsafe {
            TargetArch::before_handling_trap(&trap_type, Some(&mut *info.trap_frame));
        }
        drop(guard);

        match &trap_type {
            TrapTypes::Syscall => {
                // system call
                if self.proc().killed() {
                    self.kernel().procs().exit_current(-1, &mut self);
                }

                // An interrupt will change trap registers,
                // so don't enable until done with those registers.
                // SAFETY: Interrupt handlers has been configured properly
                unsafe { TargetArch::intr_on() };
                let syscall_no = unsafe {
                    (*self.proc().lock().deref_info().trap_frame).get_param_reg(7.into()) as i32
                };
                let res = ok_or!(self.syscall(syscall_no), usize::MAX);
                unsafe {
                    *(*self.proc().lock().deref_info().trap_frame).param_reg_mut(0.into()) = res;
                }
            }
            TrapTypes::Irq(irq_type) => unsafe {
                self.kernel().handle_irq(irq_type);
            },
            TrapTypes::BadTrap => {
                self.kernel().as_ref().write_str("usertrap(): ");

                TargetArch::print_trap_status(|arg: fmt::Arguments<'_>| {
                    self.kernel().as_ref().write_fmt(arg);
                });
                self.proc().kill();
                self.kernel().procs().exit_current(-1, &mut self);
            }
            TrapTypes::TimerInterrupt => {
                if TargetArch::cpu_id() == 0 {
                    self.kernel().clock_intr();
                }
            }
        }

        // SAFETY: It is coupled with `before_handling_trap` with same trap,
        // and trap has been handled.
        unsafe {
            TargetArch::after_handling_trap(&trap_type);
        }

        if self.proc().killed() {
            self.kernel().procs().exit_current(-1, &mut self);
        }

        // Give up the CPU if this is a timer interrupt.
        if let TrapTypes::TimerInterrupt = trap_type {
            self.yield_cpu();
        }

        unsafe { self.user_trap_ret() }
    }

    /// Return to user space.
    ///
    /// # Safety
    ///
    /// It must be called only by `user_trap`.
    pub unsafe fn user_trap_ret(self) -> ! {
        let guard = self.proc().lock();
        let info = guard.deref_info();
        // Tell trampoline.S the user page table to switch to.
        let user_table = unsafe { info.memory.assume_init_ref().page_table_addr() };
        let kstack = info.kstack;
        let trapframe = unsafe { &mut *info.trap_frame };
        drop(guard);

        // SAFETY: It is called by `user_trap_ret`, after handling the user trap.
        unsafe { TargetArch::user_trap_ret(user_table, trapframe, kstack, usertrap as usize) };
    }
}

impl KernelRef<'_, '_> {
    /// `kernel_trap` can be reached from the kernel mode, so it is a method of `Kernel`.
    unsafe fn kernel_trap(self, trap_info: usize) {
        let mut reg_backup = [0; 10];
        TargetArch::save_trap_regs(&mut reg_backup);

        assert!(
            TargetArch::is_kernel_trap(),
            "kerneltrap: not from supervisor mode"
        );
        assert!(!TargetArch::intr_get(), "kerneltrap: interrupts enabled");

        let trap_type = TargetArch::get_trap_type(trap_info);

        // SAFETY: Actually received trap with type of `trap_type`.
        unsafe {
            TargetArch::before_handling_trap(&trap_type, None);
        }
        match &trap_type {
            TrapTypes::Syscall => {
                // kernel trap cannot be a syscall.
                unreachable!()
            }
            TrapTypes::Irq(irq_type) => unsafe {
                self.handle_irq(irq_type);
            },
            TrapTypes::BadTrap => {
                self.as_ref().write_str("kerneltrap(): ");

                TargetArch::print_trap_status(|arg: fmt::Arguments<'_>| {
                    self.as_ref().write_fmt(arg);
                });
                panic!("kerneltrap");
            }
            TrapTypes::TimerInterrupt => {
                if TargetArch::cpu_id() == 0 {
                    self.clock_intr();
                }
            }
        }

        // SAFETY: It is coupled with `before_handling_trap` with same trap,
        // and trap has been handled.
        unsafe {
            TargetArch::after_handling_trap(&trap_type);
        }

        // Give up the CPU if this is a timer interrupt.
        if let TrapTypes::TimerInterrupt = trap_type {
            // TODO(https://github.com/kaist-cp/rv6/issues/517): safety?
            if let Some(ctx) = unsafe { self.get_ctx() } {
                // SAFETY:
                // Reading state without lock is safe because `proc_yield` and `sched`
                // is called after we check if current process is `RUNNING`.
                if unsafe { (*ctx.proc().info.get_mut_raw()).state } == Procstate::RUNNING {
                    ctx.yield_cpu();
                }
            }
        }

        // The yield may have caused some traps to occur,
        // so restore trap registers for use by kernelvec.S's sepc instruction.
        // SAFETY: `reg_backup` contains valid register values stored by `save_trap_regs`.
        unsafe {
            TargetArch::restore_trap_regs(&mut reg_backup);
        }
    }

    /// Handle received IRQ (only ones that needs kernel's help).
    ///
    /// # Safety
    ///
    /// It must be called only when corresponding irq has actually
    /// been received.
    unsafe fn handle_irq(self, irq_type: &IrqTypes) {
        match irq_type {
            IrqTypes::Uart => {
                // SAFETY: it's unsafe only when ctrl+p is pressed.
                unsafe { hal().console().intr(self) };
            }
            IrqTypes::Virtio => {
                hal().disk().pinned_lock().get_pin_mut().intr(self);
            }
            IrqTypes::Unknown(irq_num) => {
                // Use `panic!` instead of `println` to prevent stack overflow.
                // https://github.com/kaist-cp/rv6/issues/311
                panic!("unexpected interrupt irq={}\n", irq_num);
            }
            IrqTypes::Others(_) => {
                // do nothing
            }
        }
    }

    fn clock_intr(self) {
        let mut ticks = self.ticks().lock();
        *ticks = ticks.wrapping_add(1);
        ticks.wakeup(self);
    }
}
