#!/usr/bin/env python3

import subprocess, re, sys, glob
from collections import defaultdict

path = 'kernel-rs/**/*.rs'

if len(sys.argv) > 1:
    path = sys.argv[1]

if subprocess.call('which count-unsafe > /dev/null 2>&1', shell=True) != 0:
    print('''Please install count-unsafe by\n
`rustup update nightly && cargo +nightly install --git https://github.com/efenniht/count-unsafe`''')
    exit(-1)

if subprocess.call('which cloc > /dev/null 2>&1', shell=True) != 0:
    print('''Please install cloc by `apt install cloc`''')
    exit(-1)

space = re.compile(r'\s+')

unsafes = defaultdict(lambda: 0)

slocs = {}

for file in glob.glob(path, recursive=True):
    for line in subprocess.check_output(['count-unsafe', file], universal_newlines=True).splitlines()[1:]:
        file, begin, end, cnt, ty = line.split(',')
        unsafes[file] += int(cnt)

    stat = subprocess.check_output(['cloc', file], universal_newlines=True).splitlines()[-2].strip()
    sloc = int(space.split(stat)[-1])

    slocs[file] = sloc

    print(f'{file}: {unsafes[file]}/{sloc} = {unsafes[file]*100//sloc}%')

print('Total:')

unsafe_total = sum(unsafes.values())
sloc_total = sum(slocs.values())

print(f'{unsafe_total}/{sloc_total} = {unsafe_total*100//sloc_total}%')
