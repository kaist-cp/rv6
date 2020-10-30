#!python3

import argparse, subprocess, glob, re, collections, os
from subprocess import Popen, PIPE

parser = argparse.ArgumentParser()
parser.add_argument("range", nargs='?', default="riscv")
parser.add_argument("-t", "--trace", action="store_true")
args = parser.parse_args()

p = re.compile(rb"""
^Totals:\s+
1\s+            # files
\d+\s+          # lines
\d+\s+          # blanks
\d+\s+          # comments
(\d+)\s+        # codes
(\d+)\s+        # unsafe
""", re.VERBOSE)

counts = collections.defaultdict(lambda: (0, 0))
rust_files_union = set()

if args.trace:
    [begin, end] = args.range.split('..')
    with Popen(['git', 'rev-list', f'^{begin}~', end], stdout=PIPE) as proc:
        commits = [commit.decode('ascii').strip() for commit in proc.stdout.readlines()]
        commits.reverse()
else:
    commits = [args.range]

# TODO gather commits

for commit in commits:
    Popen(['git', 'checkout', commit]).wait()
    rust_files = glob.glob("./kernel-rs/src/*.rs")
    rust_files_union = rust_files_union.union(set(rust_files))

    for rust_file in rust_files:
        cargo_count_cwd = os.path.dirname(rust_file)
        rust_file_base = os.path.basename(rust_file)
        with Popen(['cargo', 'count', rust_file_base, '--unsafe-statistics'], stdout=PIPE, cwd=cargo_count_cwd) as proc:
            stat_line = proc.stdout.readlines()[-1]
            m = p.match(stat_line)
            if m:
                counts[(commit, rust_file)] = (int(m.group(1)), int(m.group(2)))
            else:
                print(f'Cannot count {rust_file} at {commit}.')

format_str = "{:<15} " + "{:>20} " * len(rust_files_union)
rust_files = glob.glob("./kernel-rs/src/*.rs")
for f in rust_files:
    (code, unsafe) = counts[(commits[0], f)]
    print(f + " : " + str(unsafe) + "/" + str(code) + " = " + str(format(unsafe*100/code, '.0f')) + "%")
#print(format_str.format("", *(f + counts[(commits[0], f)] if len(f) < 21 else f[:18] + '..' for f in map(os.path.basename, rust_files_union))))
#print("-" * 15 + " " + ("-" * 20 + " ") * len(rust_files_union))

# first commit doesn't contains inc/dec.
commit = commits[0]
commit_counts = [counts[(commit, f)] for f in rust_files_union]
format_args = (f'{unsafe}/{code}' for (code, unsafe) in commit_counts)
#print(format_str.format(commit[:15], *format_args))

for i in range(1, len(commits)):
    commit = commits[i]
    last_commit_counts = commit_counts
    commit_counts = [counts[(commit, f)] for f in rust_files_union]
    diff_counts = ((code2 - code1, unsafe2 - unsafe1) for ((code1, unsafe1), (code2, unsafe2)) in zip(last_commit_counts, commit_counts))

    format_args = (f'{unsafe}({dunsafe:+})/{code}({dcode:+})' for ((dcode, dunsafe), (code, unsafe)) in zip(diff_counts, commit_counts))
    #print(list(format_args))
    # print(format_str.format(commit[:20], *format_args))