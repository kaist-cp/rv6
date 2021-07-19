use crate::param::NCPU;

/// entry.S needs one stack per CPU.
#[repr(C, align(16))]
pub struct Stack([[u8; 4096]; NCPU]);

impl Stack {
    const fn new() -> Self {
        Self([[0; 4096]; NCPU])
    }
}

#[no_mangle]
pub static mut stack0: Stack = Stack::new();

/// A scratch area per CPU for machine-mode timer interrupts.
static mut TIMER_SCRATCH: [[usize; NCPU]; 5] = [[0; NCPU]; 5];

/// entry.S jumps here in machine mode on stack0.
#[no_mangle]
pub unsafe fn start() {
    unimplemented!()
}
