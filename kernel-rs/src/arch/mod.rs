use cfg_if::cfg_if;

// TODO: Can we replace all the `TargetArch` with trait `Arch`?
cfg_if! {
    if  #[cfg(target_arch = "riscv64")] {
        mod riscv;
        pub use riscv::*;
        pub type TargetArch = RiscV;
    } else if #[cfg(target_arch = "aarch64")] {
        mod arm;
        pub use arm::*;
        pub type TargetArch = Armv8;
    }
}

pub mod interface;
