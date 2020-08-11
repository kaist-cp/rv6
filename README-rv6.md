## Build

- Install toolchain.

  ```
  # Ubuntu 20.04
  sudo apt install gcc-riscv64-linux-gnu qemu-system-misc gdb-multiarch

  rustup component add rust-src
  cargo install cargo-xbuild
  ```

- Compile rv6.

  ```
  make
  ```

- Run rv6 on qemu.

  ```
  make qemu
  [to exit, C-A X]
  ```

- Debug rv6 on qemu.

  ```
  make qemu-gdb
  [to exit, C-A X]
  ```

  (Different shell)
  ```
  gdb-multiarch kernel/kernel -ex "target remote :[port]"
  # e.g., gdb-multiarch kernel/kernel -ex "target remote :29556"
  ```


## How we ported xv6 to Rust

- Run [c2rust](https://github.com/immunant/c2rust) to transpile C code to Rust.

  ```
  # Generate `compile_commands.json`.
  pip3 install scan-build
  intercept-build make qemu

  # Remove optimization flags.
  sed -i "/-O/d" compile_commands.json

  # Transpile.
  cargo +nightly-2019-12-05 install c2rust
  c2rust transpile compile_commands.json --emit-modules --emit-no-std --translate-const-macros

  # Move files.
  mv kernel/*.rs kernel-rs/src
  ```

- Enable each transpiled Rust code.

    + Add `mod <filename>;` in `kernel-rs/src/lib.rs`, and remove `$K/<filename>.o` from
      `Makefile`'s `OBJS`.
      
    + See if `rm fs.img && make qemu` and `usertests` inside qemu work fine.
