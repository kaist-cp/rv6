use crate::arch::interface::{ContextManager, ProcManager, TrapFrameManager};
use crate::arch::{asm, RiscV};
use crate::proc::RegNum;

/// A user program that calls exec("/init").
/// od -t xC initcode
const INITCODE: [u8; 52] = [
    0x17, 0x05, 0, 0, 0x13, 0x05, 0x45, 0x02, 0x97, 0x05, 0, 0, 0x93, 0x85, 0x35, 0x02, 0x93, 0x08,
    0x70, 0, 0x73, 0, 0, 0, 0x93, 0x08, 0x20, 0, 0x73, 0, 0, 0, 0xef, 0xf0, 0x9f, 0xff, 0x2f, 0x69,
    0x6e, 0x69, 0x74, 0, 0, 0x24, 0, 0, 0, 0, 0, 0, 0, 0,
];

impl ProcManager for RiscV {
    type Context = Context;
    type TrapFrame = TrapFrame;

    fn get_init_code() -> &'static [u8] {
        &INITCODE
    }

    fn cpu_id() -> usize {
        asm::cpu_id()
    }
}

/// Saved registers for kernel context switches.
#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct Context {
    pub ra: usize,
    pub sp: usize,

    /// Callee-saved
    pub s0: usize,
    pub s1: usize,
    pub s2: usize,
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,
}

/// Per-process data for the trap handling code in trampoline.S.
/// Sits in a page by itself just under the trampoline page in the
/// user page table. Not specially mapped in the kernel page table.
/// The sscratch register points here.
/// uservec in trampoline.S saves user registers in the trapframe,
/// then initializes registers from the trapframe's
/// kernel_sp, kernel_hartid, kernel_satp, and jumps to kernel_trap.
/// usertrapret() and userret in trampoline.S set up
/// the trapframe's kernel_*, restore user registers from the
/// trapframe, switch to the user page table, and enter user space.
/// The trapframe includes callee-saved user registers like s0-s11 because the
/// return-to-user path via usertrapret() doesn't return through
/// the entire kernel call stack.
#[derive(Copy, Clone)]
pub struct TrapFrame {
    /// 0 - kernel page table (satp: Supervisor Address Translation and Protection)
    pub kernel_satp: usize,

    /// 8 - top of process's kernel stack
    pub kernel_sp: usize,

    /// 16 - usertrap()
    pub kernel_trap: usize,

    /// 24 - saved user program counter (ecp: Exception Program Counter)
    pub epc: usize,

    /// 32 - saved kernel tp
    pub kernel_hartid: usize,

    /// 40
    pub ra: usize,

    /// 48
    pub sp: usize,

    /// 56
    pub gp: usize,

    /// 64
    pub tp: usize,

    /// 72
    pub t0: usize,

    /// 80
    pub t1: usize,

    /// 88
    pub t2: usize,

    /// 96
    pub s0: usize,

    /// 104
    pub s1: usize,

    /// 112
    pub a0: usize,

    /// 120
    pub a1: usize,

    /// 128
    pub a2: usize,

    /// 136
    pub a3: usize,

    /// 144
    pub a4: usize,

    /// 152
    pub a5: usize,

    /// 160
    pub a6: usize,

    /// 168
    pub a7: usize,

    /// 176
    pub s2: usize,

    /// 184
    pub s3: usize,

    /// 192
    pub s4: usize,

    /// 200
    pub s5: usize,

    /// 208
    pub s6: usize,

    /// 216
    pub s7: usize,

    /// 224
    pub s8: usize,

    /// 232
    pub s9: usize,

    /// 240
    pub s10: usize,

    /// 248
    pub s11: usize,

    /// 256
    pub t3: usize,

    /// 264
    pub t4: usize,

    /// 272
    pub t5: usize,

    /// 280
    pub t6: usize,
}

impl const TrapFrameManager for TrapFrame {
    fn set_pc(&mut self, val: usize) {
        self.epc = val;
    }

    fn set_sp(&mut self, val: usize) {
        self.sp = val;
    }

    /// Set the value of return value register
    fn set_ret_val(&mut self, val: usize) {
        self.a0 = val;
    }

    /// Set the value of function argument register
    fn param_reg_mut(&mut self, index: RegNum) -> &mut usize {
        match index {
            RegNum::R0 => &mut self.a0,
            RegNum::R1 => &mut self.a1,
            RegNum::R2 => &mut self.a2,
            RegNum::R3 => &mut self.a3,
            RegNum::R4 => &mut self.a4,
            RegNum::R5 => &mut self.a5,
            RegNum::R6 => &mut self.a6,
            RegNum::R7 => &mut self.a7,
        }
    }

    /// Get the value of function argument register
    fn get_param_reg(&self, index: RegNum) -> usize {
        match index {
            RegNum::R0 => self.a0,
            RegNum::R1 => self.a1,
            RegNum::R2 => self.a2,
            RegNum::R3 => self.a3,
            RegNum::R4 => self.a4,
            RegNum::R5 => self.a5,
            RegNum::R6 => self.a6,
            RegNum::R7 => self.a7,
        }
    }

    fn init_reg(&mut self) {
        // nothing to do
    }
}

impl const ContextManager for Context {
    fn new() -> Self {
        Self {
            ra: 0,
            sp: 0,
            s0: 0,
            s1: 0,
            s2: 0,
            s3: 0,
            s4: 0,
            s5: 0,
            s6: 0,
            s7: 0,
            s8: 0,
            s9: 0,
            s10: 0,
            s11: 0,
        }
    }

    /// Set return register (ra)
    fn set_ret_addr(&mut self, val: usize) {
        self.ra = val
    }
}
