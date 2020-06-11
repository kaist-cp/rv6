use core::sync::atomic::spin_loop_hint;

pub fn spin_loop() -> ! {
    loop {
        spin_loop_hint();
    }
}
