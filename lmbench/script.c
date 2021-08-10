#include "bench.h"

void LMBench(char *bm, char *arg1, int fd)
{
    char buf[12];
    char *args[1024];
    args[0] = bm;
    args[1] = arg1;

    execve(args[0], args, NULL);
    while (read(stderr, buf, 1) > 0)
    {
        write(fd, buf, 1);
    }
}

int main(int ac, char **av)
{

    char benchmarks[][20] = {"lat_pipe", "lat_syscall", "lat_syscall", "lat_syscall", "lat_syscall", "lat_syscall", "lat_syscall", "bw_pipe", "bw_file_rd"};
    char args[][10] = {"", "null", "read", "stat", "fstat", "open", "write", "", ""};
    int NUM_OF_BENCHMARKS = 7;
    for (int i = 0; i < NUM_OF_BENCHMARKS; i++)
    {
        int fds[2], pid, xstatus;
        if (pipe(fds) != 0)
        {
            printf(": pipe() failed\n");
            exit(1);
        }
        pid = fork();
        if (pid == 0)
        {
            close(fds[0]);
            if (strlen(args[i]) == 0) // To handle benchmarks without additional arguments
            {
                LMBench(benchmarks[i], NULL, fds[1]);
            }
            else
            {
                LMBench(benchmarks[i], args[i], fds[1]);
            }

            exit(0);
        }
        else if (pid > 0)
        {
            close(fds[1]);
            char buf[12];

            while (read(fds[0], buf, 1) > 0)
            {
                fprintf(stderr, "%c", buf[0]);
            }
            fprintf(stderr, "\n");
            close(fds[0]);
            wait(&xstatus);
        }
        else
        {
            printf("fork() failed\n");
            exit(1);
        }
    }

    exit(0);
}
