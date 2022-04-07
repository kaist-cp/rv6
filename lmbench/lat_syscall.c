/*
 * lat_syscall.c - time simple system calls
 *
 * Copyright (c) 1996 Larry McVoy.  Distributed under the FSF GPL with
 * additional restriction that results may published only if
 * (1) the benchmark is unmodified, and
 * (2) the version in the sccsid below is included in the report.
 */
char	*id = "$Id: s.lat_syscall.c 1.11 97/06/15 22:38:58-07:00 lm $\n";

#include "bench.h"
#include "kernel/fcntl.h"
#define	FNAME "./sh"

struct _state {
	int fd;
	char* file;
};

static inline uint64 read_cntpct() {
	uint64 x;
	asm volatile("isb sy");
	asm volatile("mrs %0, CNTPCT_EL0" : "=r" (x));
	asm volatile("isb sy");
	return x;
}

void
do_getppid(iter_t iterations, void *cookie)
{
	// uint64 x1, x2;
	while (iterations-- > 0) {
		// start of stage 1
		// x1 = read_cntpct();
		getppid();
		// end of total execution
		// x2 = read_cntpct();
		// printf("total: %lu\n", x2 - x1);
	}	
}

void
do_write(iter_t iterations, void *cookie)
{
	struct _state *pState = (struct _state*)cookie;
	char	c;

	while (iterations-- > 0) {
		if (write(pState->fd, &c, 1) != 1) {
			perror("/dev/null");
			return;
		}
	}
}

void
do_read(iter_t iterations, void *cookie)
{
	struct _state *pState = (struct _state*)cookie;
	char	c;

	while (iterations-- > 0) {
		if (read(pState->fd, &c, 1) != 1) {
      // TODO: find better way to replace /dev/zero.
      lseek(pState->fd, 0, SEEK_SET);
      continue;
			// perror("/dev/zero");
			// return;
		}
	}
}

void
do_stat(iter_t iterations, void *cookie)
{
	struct _state *pState = (struct _state*)cookie;
	struct	stat sbuf;

	while (iterations-- > 0) {
		if (stat(pState->file, &sbuf) == -1) {
			perror(pState->file);
			return;
		}
	}
}

void
do_fstat(iter_t iterations, void *cookie)
{
	struct _state *pState = (struct _state*)cookie;
	struct	stat sbuf;

	while (iterations-- > 0) {
		if (fstat(pState->fd, &sbuf) == -1) {
			perror("fstat");
			return;
		}
	}
}

void
do_openclose(iter_t iterations, void *cookie)
{
	struct _state *pState = (struct _state*)cookie;
	int	fd;

	while (iterations-- > 0) {
		fd = open(pState->file, 0);
		if (fd == -1) {
			perror(pState->file);
			return;
		}
		close(fd);
	}
}

int
main(int ac, char **av)
{
	int parallel = 1;
	int warmup = 0;
	int repetitions = -1;
	int c;
	struct _state state;
	char* usage = "[-P <parallelism>] [-W <warmup>] [-N <repetitions>] null|read|write|stat|fstat|open [file]\n";

	while (( c = getopt(ac, av, "P:W:N:")) != EOF) {
		switch(c) {
		case 'P':
			parallel = atoi(optarg);
			if (parallel <= 0) lmbench_usage(ac, av, usage);
			break;
		case 'W':
			warmup = atoi(optarg);
			break;
		case 'N':
			repetitions = atoi(optarg);
			break;
		default:
			lmbench_usage(ac, av, usage);
			break;
		}
	}
	if (optind != ac - 1 && optind != ac - 2 ) {
		lmbench_usage(ac, av, usage);
	}
	
	state.file = FNAME;
	if (optind == ac - 2) 
		state.file = av[optind + 1];

	if (!strcmp("null", av[optind])) {
		benchmp(NULL, do_getppid, NULL, 0, parallel, 
			warmup, repetitions, &state);
		micro("Simple syscall", get_n());
	} else if (!strcmp("write", av[optind])) {
    // TODO: find better way to replace /dev/null.
		// state.fd = open("/dev/null", 1);
    state.fd = 1;
		benchmp(NULL, do_write, NULL, 0, parallel, 
			warmup, repetitions, &state);
		printf("\n");
		micro("Simple write", get_n());
		close(state.fd);
	} else if (!strcmp("read", av[optind])) {

    // TODO: find better way to replace /dev/zero.
    state.fd = open("./dev_zero", O_CREATE|O_RDWR);
    for (int i=0; i<10000; i++) {
      write(state.fd, "0" ,1);
    }
    lseek(state.fd, 0, SEEK_SET);

		// state.fd = open("/dev/zero", 0);
		if (state.fd == -1) {
			fprintf(stderr, "Simple read: -1\n");
			return(1);
		}
		benchmp(NULL, do_read, NULL, 0, parallel, 
			warmup, repetitions, &state);
		micro("Simple read", get_n());
		close(state.fd);
	} else if (!strcmp("stat", av[optind])) {
		benchmp(NULL, do_stat, NULL, 0, parallel, 
			warmup, repetitions, &state);
		micro("Simple stat", get_n());
	} else if (!strcmp("fstat", av[optind])) {
		state.fd = open(state.file, 0);
		benchmp(NULL, do_fstat, NULL, 0, parallel, 
			warmup, repetitions, &state);
		micro("Simple fstat", get_n());
		close(state.fd);
	} else if (!strcmp("open", av[optind])) {
		benchmp(NULL, do_openclose, NULL, 0, parallel, 
			warmup, repetitions, &state);
		micro("Simple open/close", get_n());
	} else {
		lmbench_usage(ac, av, usage);
	}
	exit(0);
}
