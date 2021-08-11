#include "bench.h"

void LMBench(char *bm, char *arg1)
{
    char *args[1024];
    args[0] = bm;
    args[1] = arg1;

    execve(args[0], args, NULL);
}

int main(int ac, char **av)
{

    char benchmarks[][20] = {"lat_ctx","lat_proc", /* "lat_proc",*/ "lat_proc", "lat_pipe", "lat_syscall", "lat_syscall", "lat_syscall", "lat_syscall", "lat_syscall", "lat_syscall", "bw_pipe", "bw_file_rd"};
    char args[][30] = {"2 4", "fork", /* "exec",*/ "shell", "", "null", "read", "stat", "fstat", "open", "write", "", "512 open2close README"};
    int NUM_OF_BENCHMARKS = 12;
    for (int i = 0; i < NUM_OF_BENCHMARKS; i++)
    {
        int pid, xstatus;
        pid = fork();
        if (pid == 0)
        {
            if (strlen(args[i]) == 0) // To handle benchmarks without additional arguments
            {
                LMBench(benchmarks[i], NULL);
            }
            else
            {
                LMBench(benchmarks[i], args[i]);
            }

            exit(0);
        }
        else if (pid > 0)
        {
            fprintf(stderr, "\n");
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
