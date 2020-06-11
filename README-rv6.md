- Install toolchain.

  ```
  # Ubuntu 20.04
  sudo apt install gcc-riscv64-linux-gnu qemu-system-misc

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
