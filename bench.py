#!/usr/bin/env python3

import os, argparse, datetime, time, statistics, subprocess

parser = argparse.ArgumentParser(description='usertests benchmark')
parser.add_argument('-i', '--iter', type=int, default=10, help='number of execcountations. Default = 10')
parser.add_argument('-o', '--output', type=str, default='bench.result', help='benchmark result path. Default = bench.result')
parser.add_argument('-e', '--execcount', type=int, default=10, help='number of executions per execcountation for each testcase. Default=10')
parser.add_argument('-c', '--case', type=str, default="", help='index of testcase to be executed')
parser.add_argument('-t', '--timemode', type=str, default="cpu-clock", help='time measurment scale: cpu-clock | wall-clock. Default = cpu-clock')
parser.add_argument('-v', '--verbose', type=bool, default=False, help='write detailed information to the result. Default =False')
parser.add_argument('--option', type=str, default='RUST_MODE=release', help='make option')


tmpfile = '_bench.tmp'

def main(args):
    compile_args = f'ITER={args.execcount} USERTEST=yes BENCH=yes CASE={args.case} {args.option}'

    if args.timemode != 'cpu-clock' and args.timemode != 'wall-clock':
        print('timemode must be "cpu-clock" or "wall-clock')
        exit(1)        

    with open(args.output, 'a', buffering=1) as f:
        stat = []
        f.write('Start benchmark {}\n'.format(datetime.datetime.now()))
        try:
            subprocess.check_call('make clean', shell=True)
        except Exception:
            print("")
        subprocess.check_call(f'make kernel/kernel {compile_args}', shell=True)
        subprocess.check_call(f'make fs.img {compile_args}', shell=True)

        for n in range(args.iter):
            begin = time.perf_counter()
            subprocess.check_call(f'make qemu {compile_args} 2>/dev/null > {tmpfile}', shell=True)
            elapsed = time.perf_counter() - begin
            if args.timemode == 'wall-clock':
                f.write(f'{elapsed}\n')
                stat.append(elapsed)

            results = {}
            if args.timemode == 'cpu-clock':
                with open(f'{tmpfile}', 'r') as f2:
                    for line in f2:
                        if line[0:5] == 'Test=':
                            if args.verbose:
                                f.write(line)
                            tokens = line.split(',')
                            test_name = tokens[0][5:]
                            elapsed = int(tokens[1].split('=')[-1].strip())
                            if not test_name in results:
                                results[test_name] = []
                            results[test_name].append(elapsed)
                for test_name in results:
                    if len(results[test_name]) == 1:
                        mean = results[test_name][0]
                    else:
                        mean = statistics.mean(results[test_name])
                    std = statistics.stdev(results[test_name])
                    f.write(f"Test={test_name}, Iter={n}, ExecCount={args.execcount}, Mean={mean}, Standard Deviation={std}\n")

            os.remove('fs.img')
            os.remove(f'{tmpfile}')
            subprocess.check_call(f'make fs.img {compile_args}', shell=True)

        if args.timemode == 'wall-clock':
            if len(stat) > 1:
                avg = statistics.mean(stat)
                std = statistics.stdev(stat)
            else:
                avg = stat[0]
                std = 0

            f.write(f'duration = {stat[0]}\n')
            f.write(f'Mean={avg}, Standard Deviation={std}, N={args.number}, Iter={args.execcount}\n')
            f.write('Finish benchmark\n')

if __name__ == "__main__":
    main(parser.parse_args())
