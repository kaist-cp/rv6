## Build

- Install toolchain.

  ```
  # Ubuntu 20.04
  sudo apt install gcc-riscv64-linux-gnu qemu-system-misc gdb-multiarch

  rustup component add rust-src
  ```

- Compile rv6.

  ```
  make
  ```

- Run rv6 on qemu (RISC-V).

  ```
  make qemu
  [to exit, C-A X]
  ```

- Run rv6 on qemu (Armv8).

  ```
  TARGET=arm make qemu
  [to exit, C-A X]
  ```

- Run with specified version of GIC (ARM Generic Interrupt Controller) (only on ARM)
  ```
  TARGET=arm GIC_VERSION=2 make qemu // default
  TARGET=arm GIC_VERSION=3 make qemu
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

## Benchmark

Run `bench.py` with options.

```sh
usage: bench.py [-h] [-i ITER] [-o OUTPUT] [-e EXECCOUNT] [-c CASE] [-t TIMEMODE] [-v VERBOSE] [--option OPTION]

usertests benchmark

optional arguments:
  -h, --help            show this help message and exit
  -i ITER, --iter ITER  number of iterations. Default = 10
  -o OUTPUT, --output OUTPUT
                        benchmark result path. Default = bench.result
  -e EXECCOUNT, --execcount EXECCOUNT
                        number of executions per iteration for each testcase. Default=10
  -c CASE, --case CASE  index of testcase to be executed
  -t TIMEMODE, --timemode TIMEMODE
                        time measurment scale: cpu-clock | wall-clock. Default = cpu-clock
  -v VERBOSE, --verbose VERBOSE
                        write detailed information to the result. Default =False
  --option OPTION       make option
```

For the experiment, we used the following options:

```sh
./bench.py -i 1 -e 10 -t cpu-clock
```

You can see the result in `bench.result`. An exemplary output is:

```
Start benchmark 2021-01-21 16:01:02.001441
Test=manywrites, Iter=0, ExecCount=10, Mean=10250831503.7, Standard Deviation=310099929.7603766
Test=execout, Iter=0, ExecCount=10, Mean=234127979491.5, Standard Deviation=2063096239.6600628
Test=copyin, Iter=0, ExecCount=10, Mean=72843513.2, Standard Deviation=2593483.998048691
Test=copyout, Iter=0, ExecCount=10, Mean=22197443.3, Standard Deviation=508065.61895619787
Test=copyinstr1, Iter=0, ExecCount=10, Mean=17590118.2, Standard Deviation=724335.8052858264
Test=copyinstr2, Iter=0, ExecCount=10, Mean=31026224.2, Standard Deviation=1277484.7001188442
Test=copyinstr3, Iter=0, ExecCount=10, Mean=22767261.8, Standard Deviation=850377.8462254686
Test=rwsbrk, Iter=0, ExecCount=10, Mean=52610103.9, Standard Deviation=3796969.8039983716
Test=truncate1, Iter=0, ExecCount=10, Mean=72626663.6, Standard Deviation=3588155.194589591
Test=truncate2, Iter=0, ExecCount=10, Mean=57515131, Standard Deviation=2888724.774054423
Test=truncate3, Iter=0, ExecCount=10, Mean=699796084.9, Standard Deviation=8685616.881588288
Test=reparent2, Iter=0, ExecCount=10, Mean=35125494097.1, Standard Deviation=499343605.2572245
Test=pgbug, Iter=0, ExecCount=10, Mean=16853126.3, Standard Deviation=576202.2373050493
Test=sbrkbugs, Iter=0, ExecCount=10, Mean=57242485.4, Standard Deviation=1375587.7073186815
Test=badarg, Iter=0, ExecCount=10, Mean=16732117744.5, Standard Deviation=314298214.1929783
Test=reparent, Iter=0, ExecCount=10, Mean=4694825008.8, Standard Deviation=53248537.52366887
Test=twochildren, Iter=0, ExecCount=10, Mean=23553479262.2, Standard Deviation=312654479.1470448
Test=forkfork, Iter=0, ExecCount=10, Mean=4140333092.8, Standard Deviation=42441996.98412191
Test=forkforkfork, Iter=0, ExecCount=10, Mean=11070152007.5, Standard Deviation=4214247.017523988
Test=argptest, Iter=0, ExecCount=10, Mean=19669825.3, Standard Deviation=1076447.8157422373
Test=createdelete, Iter=0, ExecCount=10, Mean=1692106081.6, Standard Deviation=43254490.91422707
Test=linkunlink, Iter=0, ExecCount=10, Mean=738579118.6, Standard Deviation=17181223.968526576
Test=linktest, Iter=0, ExecCount=10, Mean=122194298.2, Standard Deviation=8043328.834906379
Test=unlinkread, Iter=0, ExecCount=10, Mean=112391973.3, Standard Deviation=5462054.9097064
Test=concreate, Iter=0, ExecCount=10, Mean=5555714294.8, Standard Deviation=86235817.9635743
Test=subdir, Iter=0, ExecCount=10, Mean=317356618.3, Standard Deviation=6779472.5084936945
Test=fourfiles, Iter=0, ExecCount=10, Mean=364562909.2, Standard Deviation=18523538.444742586
Test=sharedfd, Iter=0, ExecCount=10, Mean=1258305490.5, Standard Deviation=99211093.82524228
Test=dirtest, Iter=0, ExecCount=10, Mean=62557539.1, Standard Deviation=5084440.4897059435
Test=exectest, Iter=0, ExecCount=10, Mean=101016489.8, Standard Deviation=3849253.3196121096
Test=bigargtest, Iter=0, ExecCount=10, Mean=86731337.4, Standard Deviation=1234924.8689326271
Test=bigwrite, Iter=0, ExecCount=10, Mean=2179088452.5, Standard Deviation=47737862.418833666
Test=bsstest, Iter=0, ExecCount=10, Mean=16703757.3, Standard Deviation=1191501.0847995295
Test=sbrkbasic, Iter=0, ExecCount=10, Mean=14768144087.8, Standard Deviation=122624404.54519753
Test=sbrkmuch, Iter=0, ExecCount=10, Mean=11604135700.4, Standard Deviation=160586718.12640956
Test=kernmem, Iter=0, ExecCount=10, Mean=590334800.2, Standard Deviation=23889526.48877363
Test=sbrkfail, Iter=0, ExecCount=10, Mean=53275587805.5, Standard Deviation=501197941.33818287
Test=sbrkarg, Iter=0, ExecCount=10, Mean=75074295, Standard Deviation=6985498.194494156
Test=validatetest, Iter=0, ExecCount=10, Mean=151271691.7, Standard Deviation=15584963.951729957
Test=stacktest, Iter=0, ExecCount=10, Mean=33503426, Standard Deviation=1647149.0807118826
Test=opentest, Iter=0, ExecCount=10, Mean=25377926.3, Standard Deviation=1436404.7789807292
Test=writetest, Iter=0, ExecCount=10, Mean=1000989926.6, Standard Deviation=47432019.17070772
Test=writebig, Iter=0, ExecCount=10, Mean=2151370834.9, Standard Deviation=126541151.0858887
Test=createtest, Iter=0, ExecCount=10, Mean=1952230863.1, Standard Deviation=62090935.928086035
Test=openiput, Iter=0, ExecCount=10, Mean=339365750.1, Standard Deviation=71608476.42326514
Test=exitiput, Iter=0, ExecCount=10, Mean=74096425.7, Standard Deviation=1950034.465084602
Test=iput, Iter=0, ExecCount=10, Mean=62158786.4, Standard Deviation=2645237.072454994
Test=mem, Iter=0, ExecCount=10, Mean=16648525535.2, Standard Deviation=142852748.4777774
Test=pipe1, Iter=0, ExecCount=10, Mean=36972031.7, Standard Deviation=1142602.1014287276
Test=killstatus, Iter=0, ExecCount=10, Mean=36911937347.1, Standard Deviation=28681127.996621083
Test=preempt, Iter=0, ExecCount=10, Mean=585400609.7, Standard Deviation=181312782.38400963
Test=exitwait, Iter=0, ExecCount=10, Mean=1175053781.1, Standard Deviation=9824044.958759846
Test=rmdot, Iter=0, ExecCount=10, Mean=72308323, Standard Deviation=2467978.045960656
Test=fourteen, Iter=0, ExecCount=10, Mean=140964120.7, Standard Deviation=3282846.848378064
Test=bigfile, Iter=0, ExecCount=10, Mean=216833686.3, Standard Deviation=16241193.844760058
Test=dirfile, Iter=0, ExecCount=10, Mean=93530890.3, Standard Deviation=10745218.64240552
Test=iref, Iter=0, ExecCount=10, Mean=1965505009, Standard Deviation=61145641.97305806
Test=forktest, Iter=0, ExecCount=10, Mean=788582250.6, Standard Deviation=10910378.262444843
Test=bigdir, Iter=0, ExecCount=10, Mean=40395690096, Standard Deviation=2016582586.665442
```

LMbench benchmark

you can execute some [LMbench](http://lmbench.sourceforge.net) mircrobenchmarks ported for rv6.

```
# lmbench is only supported on ARM now.
TARGET=arm RUST_MODE=release make qemu
$ lmbench
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
