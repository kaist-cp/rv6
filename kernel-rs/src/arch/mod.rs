use cfg_if::cfg_if;

cfg_if! {
    if  #[cfg(target_arch = "riscv64")] {
        mod riscv;
        pub use riscv::*;
    } else if #[cfg(target_arch = "aarch64")] {
        mod arm;
        pub use arm::*;
    }
}

pub mod interface;
