#!/usr/bin/env python3

import subprocess, glob, re, os
from subprocess import Popen, PIPE

if os.popen('cargo --list | grep count').read()=="":
    print('Please install cargo-count by `cargo install cargo-count`.\nIf it doesn\'t work, check https://github.com/kbknapp/cargo-count#compiling')
    exit(0)

p = re.compile(rb"""
^Totals:\s+
1\s+            # files
\d+\s+          # lines
\d+\s+          # blanks
\d+\s+          # comments
(\d+)\s+        # codes
(\d+)\s+        # unsafe
""", re.VERBOSE)

rust_files = glob.glob("./kernel-rs/src/**/*.rs", recursive=True)

for rust_file in rust_files:
    cargo_count_cwd = os.path.dirname(rust_file)
    rust_file_base = os.path.basename(rust_file)
    with Popen(['cargo', 'count', rust_file_base, '--unsafe-statistics'], stdout=PIPE, cwd=cargo_count_cwd) as proc:
        stat_line = proc.stdout.readlines()[-1]
        m = p.match(stat_line)
        if m:
            (code, unsafe) = (int(m.group(1)), int(m.group(2)))
            print(rust_file + " : " + str(unsafe) + "/" + str(code) + " = " + str(unsafe*100//code) + "%")
        else:
            print(f'Cannot count {rust_file_base} at riscv.')

print("\n>>> Unsafe statistics for rv6")
os.system('cargo count -a -l rs --unsafe-statistics')
exit(0)
