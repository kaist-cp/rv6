// HACK(@efenniht): Block inlining to avoid an infinite loop miscompilation of LLVM:
// https://github.com/rust-lang/rust/issues/28728.
#[inline(never)]
pub fn spin_loop() -> ! {
    loop {
        ::core::hint::spin_loop();
    }
}
