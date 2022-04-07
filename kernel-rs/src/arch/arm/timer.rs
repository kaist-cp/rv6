use cortex_a::{asm::barrier, registers::*};
use tock_registers::interfaces::{Readable, Writeable};

use crate::arch::{interface::TimeManager, Armv8};

const US_PER_S: u64 = 1_000_000;

const TIMER_TICK_MS: u64 = 100;

// pub struct Timer;

impl TimeManager for Armv8 {
    fn timer_init() {
        let mut x: usize;

        // for user-space clock time profiling.
        unsafe {
            asm!("mrs {}, cntkctl_el1", out(reg) x);
            x &= !((3 << 8) | (1 << 1));
            x |= 1;
            asm!("msr cntkctl_el1, {}", in(reg) x);
        }

        set_next_timer();
    }

    fn uptime_as_micro() -> Result<usize, ()> {
        Ok((read_cntpct() * US_PER_S / read_freq()) as usize)
    }

    fn r_cycle() -> usize {
        read_cntpct() as usize
    }
}

pub fn read_cntpct() -> u64 {
    // Prevent that the counter is read ahead of time due to out-of-order execution.
    unsafe { barrier::isb(barrier::SY) };
    let ret = CNTPCT_EL0.get();
    unsafe { barrier::isb(barrier::SY) };
    ret
}

pub fn read_freq() -> u64 {
    unsafe { barrier::isb(barrier::SY) };
    CNTFRQ_EL0.get()
}

pub fn set_next_timer() {
    unsafe { barrier::isb(barrier::SY) };
    let freq = CNTFRQ_EL0.get();
    let count = TIMER_TICK_MS * freq / 1000;

    unsafe { barrier::isb(barrier::SY) };
    CNTV_TVAL_EL0.set(count);
    CNTV_CTL_EL0.write(CNTV_CTL_EL0::ENABLE.val(1) + CNTV_CTL_EL0::IMASK.val(0));
    unsafe { barrier::isb(barrier::SY) };
}

pub fn udelay(us: u32) {
    let mut current = read_cntpct();
    let condition = current + read_freq() * us as u64 / 1000000;

    while condition > current {
        current = read_cntpct();
    }
}
