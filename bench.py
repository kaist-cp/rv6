import os, argparse, datetime, time, numpy

parser = argparse.ArgumentParser(description='usertests benchmark')
parser.add_argument('-n', '--number', type=int, default=10, help='number of usertests')
parser.add_argument('-o', '--output', type=str, default='bench.result', help='benchmark result path')
parser.add_argument('--option', type=str, default='RUST_MODE=release OPTFLAGS=-O3', help='make option')

def main(args):
    stat = []

    f = open(args.output, 'a', buffering=1)
    f.write('Start benchmark {}\n'.format(datetime.datetime.now()))
    
    os.system('make clean')
    os.system(f'make kernel/kernel USERTEST=yes {args.option}')
    os.system(f'make fs.img USERTEST=yes {args.option}')

    for _ in range(args.number):
        begin = time.perf_counter()
        os.system(f'make qemu USERTEST=yes {args.option} 2>/dev/null')
        elapsed = time.perf_counter() - begin
        f.write(f'{elapsed}\n')
        stat.append(elapsed)

        os.remove('fs.img')
        os.system(f'make fs.img USERTEST=yes {args.option}')

    avg = numpy.average(stat)
    std = numpy.std(stat)

    f.write(f'Mean={avg}, Standard Deviation={std}, N={args.number}\n')
    f.write('Finish benchmark\n')

    f.close()

main(parser.parse_args())
