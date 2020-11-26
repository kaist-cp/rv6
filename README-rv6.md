## Build

- Install toolchain.

  ```
  # Ubuntu 20.04
  # The latest qemu-system-misc(1:4.2-3ubuntu6.9) can't boot rv6, so please use older version (1:4.2-3ubuntu6)
  sudo apt install gcc-riscv64-linux-gnu qemu-system-misc=1:4.2-3ubuntu6 gdb-multiarch

  rustup component add rust-src
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

  - Run rv6 under QEMU and enable remote debugging

  ```
  make qemu-gdb
  [to exit, C-A X]
  ```

  - Check GDB PORT

  ![gdb_port](https://imgur.com/3qCU06N.png)

  - Start GDB and connect it to QEMU's waiting remote debugging stub

  ```
  (In another shell)
  gdb-multiarch kernel/kernel -ex "target remote :[port]"
  # e.g., gdb-multiarch kernel/kernel -ex "target remote :27214"
  [to exit, C-D]
  ```

- Useful gdb commands for rv6
  
  - **All these gdb commands should be typed in the second shell which executed `gdb-multiarch`**

  - `[b]reak <function name or filename:line# or *memory address>` : Set a breakpoint
  
    + **`[b]reak` means that you can type either of `b` and `break`**

    + Set breakpoint to `kinit()`
    
      ```
      b kinit
      ```
    
    + Set breakpoint to specific address (e.g., `0x8049000`)

      ```
      b *0x8049000
      ```
    
    + Set breakpoint to specific instruction of `kinit()` : e.g., `jalr -314(ra)` in `kinit()`

      * Type `[disas]semble kinit` to check the offset of the instruction to break.

      ![break_offset](https://imgur.com/nn6FBG4.png)
      
      * Set breakpoint to offset from function
      
      ```
      b *(kinit+54)
      ```

      * Note that `b *0x800194ba` also sets breakpoint to `kinit+54`

  - `[c]ontinue` : Resumes execution of a stopped program, stopping again at the next breakpoint

  - `[i]nfo (about)` : Lists information about the argument (about)

    + `[i]nfo [r]egisters` : Lists the contents of each register

      * `i r $s0` : List only the content of register `$s0`

    + `[i]nfo [var]iables` : Lists global/static variables with addresses

    + `[i]nfo [b]reakpoints` : Show address of each breakpoint and whether it is enabled.

    ![info_var](https://i.imgur.com/HTn1rTw.png)

    + `[maint]enance [i]nfo [sec]tions` : Get extra information about program sections. It shows what `kernel/kernel.ld` does in rv6.

  - `[d]elete <breakpoint #>` : Removes the indicated breakpoint. To see breakpoint numbers, run `i b`
  
    + e.g., `d 3`
  
  - `[n]ext` : Steps through a single line of (Rust) code. Steps **over** function calls

    + `n x` : Steps x lines of code. e.g., `n 3`

  - `[n]ext[i]` : Steps through a single x86 instruction. Steps **over** calls

    + `ni x` : Steps x lines of code. e.g., `ni 3`
  
  - `[s]tep` : Steps through a single line of (Rust) code. Steps **into** function calls

  - `[s]tep[i]` : Steps through a single x86 instruction. Steps **into** calls

  - `[disas]semble <function name>` : Disassembles a function into assembly. e.g., `disas kinit`

    + Typing `disas` disassembles assembly of PC
  
  - `[p]rint <expression>` : Prints the value which the indicated expression evaluates to.

    + e.g., print various expressions in `spinlock.rs::acquire()`

    ![print](https://i.imgur.com/8OtkOig.png)

  - `[x]/(number)(format)(unit_size) <address>` : Examines the data located in memory at address.

    + e.g., `x/4x p`, `x/4x 0x80062000`

    ![x/4x](https://i.imgur.com/Gb4N3KB.png)
  
  - Enable Text User Interface(TUI)

    + Using TUI helps to understand Rust code

    + Type `tui enable` to enable TUI

    ![tui_enable](https://imgur.com/OHmmscQ.png)

    + Type `tui disable` to enable TUI

    + Check https://sourceware.org/gdb/current/onlinedocs/gdb/TUI.html#TUI

  - Tips for debugging rv6

    + If rv6 doesn't boot, set the breakpoint to `kernel_main()` with `b kernel::kernel_main` and execute each line with `n`.

    + If an infinite loop occurs during a usertest, check C code of the usertest in `user/usertests.c` first. Then set breakpoints to appropriate functions. For example, if infinite loop occurs during a `forkforkfork` test, set breakpoints to `sys_unlink()`, `sys_fork`, etc.

    + It may be necessary to check whether the struct's field has an appropriate value (`print cpu.id`) or whether the address of the variable is appropriate (`print &(cpu.id)`). The x command (e.g., `x/4x 0x8049000`) allows you to examine memory (of the address of the variable) even after the variable is optimized out.
  
  - .gdbinit

    + `.gdbinit` is generated after you run `make qemu-gdb`. This file contains GDB commands to automatically execute during GDB startup. The contents of the file are as follows :

    ```
    set confirm off
    set architecture riscv:rv64
    target remote 127.0.0.1:[PORT]
    symbol-file kernel/kernel
    set disassemble-next-line auto
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
