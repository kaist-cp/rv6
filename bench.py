import os, argparse, datetime, time, statistics, subprocess

parser = argparse.ArgumentParser(description='usertests benchmark')
parser.add_argument('-n', '--number', type=int, default=10, help='number of usertests')
parser.add_argument('-o', '--output', type=str, default='bench.result', help='benchmark result path')
parser.add_argument('--option', type=str, default='RUST_MODE=release OPTFLAGS=-O3', help='make option')

def main(args):
    stat = []

    with open(args.output, 'a', buffering=1) as f:
        f.write('Start benchmark {}\n'.format(datetime.datetime.now()))

        subprocess.check_call('make clean', shell=True)
        subprocess.check_call(f'make kernel/kernel USERTEST=yes {args.option}', shell=True)
        subprocess.check_call(f'make fs.img USERTEST=yes {args.option}', shell=True)

        for _ in range(args.number):
            begin = time.perf_counter()
            subprocess.check_call(f'make qemu USERTEST=yes {args.option} 2>/dev/null', shell=True)
            elapsed = time.perf_counter() - begin
            f.write(f'{elapsed}\n')
            stat.append(elapsed)

            os.remove('fs.img')
            subprocess.check_call(f'make fs.img USERTEST=yes {args.option}', shell=True)

        avg = statistics.mean(stat)
        std = statistics.stdev(stat)

        f.write(f'Mean={avg}, Standard Deviation={std}, N={args.number}\n')
        f.write('Finish benchmark\n')


if __name__ == "__main__":
    main(parser.parse_args())
