use crate::{
    kernel::{kernel_ref, KernelRef},
    proc::{kernel_ctx, KernelCtx},
};

// TODO: replace these
// extern "C" {
//     // trampoline.S
//     static mut trampoline: [u8; 0];

//     static mut uservec: [u8; 0];

//     static mut userret: [u8; 0];

//     // In kernelvec.S, calls kerneltrap().
//     fn kernelvec();
// }

extern "C" {
    // trap_asm.S
    fn trapret() -> !;
}

pub fn trapinit() {}

/// Set up to take exceptions and traps while in the kernel.
pub unsafe fn trapinitcore() {
    // nothing to do
    // unsafe { w_stvec(kernelvec as _) };
}

/// Handle an interrupt, exception, or system call from user space.
/// Called from trampoline.S.
#[no_mangle]
pub unsafe extern "C" fn usertrap() {
    // SAFETY
    // * usertrap can be reached only after the initialization of the kernel.
    // * It's the beginning of this thread, so there's no exsiting `KernelCtx` or `CurrentProc`.
    unsafe { kernel_ctx(|ctx| ctx.user_trap()) };
}

#[no_mangle]
pub unsafe extern "C" fn swi_handler() {
    // SAFETY
    // TODO
    unsafe {
        asm!("nop");
    }
    // unsafe { kernel_ctx(|ctx| ctx.swi_handler())};
    // unsafe { kernel_ctx(|ctx| ctx.user_trap()) };
}

#[no_mangle]
pub unsafe extern "C" fn irq_handler() {
    // SAFETY
    // TODO
    unsafe {
        asm!("nop");
    }
    // unsafe { kernel_ctx(|ctx| ctx.swi_handler())};
    // unsafe { kernel_ctx(|ctx| ctx.user_trap()) };
}

#[no_mangle]
pub unsafe extern "C" fn dabort_handler() {
    // SAFETY
    // TODO
    unsafe {
        asm!("nop");
    }
    // unsafe { kernel_ctx(|ctx| ctx.swi_handler())};
    // unsafe { kernel_ctx(|ctx| ctx.user_trap()) };
}

#[no_mangle]
pub unsafe extern "C" fn iabort_handler() {
    // SAFETY
    // TODO
    unsafe {
        asm!("nop");
    }
    // unsafe { kernel_ctx(|ctx| ctx.swi_handler())};
    // unsafe { kernel_ctx(|ctx| ctx.user_trap()) };
}

#[no_mangle]
pub unsafe extern "C" fn reset_handler() {
    // SAFETY
    // TODO
    unsafe {
        asm!("nop");
    }
    // unsafe { kernel_ctx(|ctx| ctx.swi_handler())};
    // unsafe { kernel_ctx(|ctx| ctx.user_trap()) };
}

#[no_mangle]
pub unsafe extern "C" fn und_handler() {
    // SAFETY
    // TODO
    unsafe {
        asm!("nop");
    }
    // unsafe { kernel_ctx(|ctx| ctx.swi_handler())};
    // unsafe { kernel_ctx(|ctx| ctx.user_trap()) };
}

#[no_mangle]
pub unsafe extern "C" fn na_handler() {
    // SAFETY
    // TODO
    unsafe {
        asm!("nop");
    }
    // unsafe { kernel_ctx(|ctx| ctx.swi_handler())};
    // unsafe { kernel_ctx(|ctx| ctx.user_trap()) };
}

#[no_mangle]
pub unsafe extern "C" fn fiq_handler() {
    // SAFETY
    // TODO
    unsafe {
        asm!("nop");
    }
    // unsafe { kernel_ctx(|ctx| ctx.user_trap()) };
}

#[no_mangle]
pub unsafe extern "C" fn bad_handler() {
    // SAFETY
    // TODO
    unsafe {
        asm!("nop");
    }
    // unsafe { kernel_ctx(|ctx| ctx.user_trap()) };
}

#[no_mangle]
pub unsafe extern "C" fn error_handler() {
    todo!()
}

#[no_mangle]
pub unsafe extern "C" fn default_handler() {
    todo!()
}

/// Interrupts and exceptions from kernel code go here via kernelvec,
/// on whatever the current kernel stack is.
#[no_mangle]
pub unsafe fn kerneltrap() {
    // SAFETY: kerneltrap can be reached only after the initialization of the kernel.
    unsafe { kernel_ref(|kref| kref.kernel_trap()) };
}

impl KernelCtx<'_, '_> {
    /// `user_trap` can be reached only from the user mode, so it is a method of `KernelCtx`.
    unsafe fn user_trap(self) -> ! {
        todo!()
    }

    /// Return to user space.
    pub unsafe fn user_trap_ret(self) -> ! {
        todo!()
    }
}

impl KernelRef<'_, '_> {
    /// `kernel_trap` can be reached from the kernel mode, so it is a method of `Kernel`.
    unsafe fn kernel_trap(self) {
        todo!()
    }

    fn clock_intr(self) {
        let mut ticks = self.ticks().lock();
        *ticks = ticks.wrapping_add(1);
        ticks.wakeup(self);
    }

    /// Check if it's an external interrupt or software interrupt,
    /// and handle it.
    /// Returns 2 if timer interrupt,
    /// 1 if other device,
    /// 0 if not recognized.
    unsafe fn dev_intr(self) -> i32 {
        todo!()
    }
}
