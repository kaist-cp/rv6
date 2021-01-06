#!/usr/bin/env python3

import os, re, sys, glob
from collections import defaultdict

path = 'kernel-rs/**/*.rs'

if len(sys.argv) > 1:
    path = sys.argv[1]

if os.system('which count-unsafe > /dev/null 2>&1') != 0:
    print('''Please install count-unsafe by\n
`rustup update nightly && cargo +nightly install --git https://github.com/efenniht/count-unsafe`''')
    exit(-1)

if os.system('which cloc > /dev/null 2>&1') != 0:
    print('''Please install cloc by `apt install cloc`''')
    exit(-1)

space = re.compile(r'\s+')

unsafes = defaultdict(lambda: 0)

for line in os.popen(f'count-unsafe {path}').readlines()[1:]:
    file, begin, end, cnt, ty = line.split(',')

    unsafes[file] += int(cnt)

slocs = {}

for file in glob.glob(path, recursive=True):
    stat = os.popen(f'cloc {file}').readlines()[-2].strip()
    sloc = int(space.split(stat)[-1])

    slocs[file] = sloc

    print(f'{file} : {unsafes[file]}/{sloc} = {unsafes[file]*100//sloc}')

print('Total:')

print(f'{sum(unsafes.values())}/{sum(slocs.values())}')
