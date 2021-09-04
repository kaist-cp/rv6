#include "bench.h"

#ifndef ITER
#define ITER 1
#endif

int main(int ac, char **av)
{
    char *benchmarks[13][4] = {{"lat_syscall", "null", "", ""}, {"lat_syscall", "read", "", ""}, {"lat_syscall", "stat", "", ""}, {"lat_syscall", "fstat", "", ""}, {"lat_syscall", "open", "", ""}, {"lat_syscall", "write", "", ""}, {"lat_proc", "fork", "", ""}, {"lat_proc", "shell", "", ""}, {"lat_pipe", "", "", ""}, {"lat_ctx", "2", "", ""}, {"bw_pipe", "", "", ""}, {"bw_file_rd", "512", "open2close", "README"}, {"bw_file_rd", "512", "io_only", "README"}};
    for (int j=0; j<ITER; j++) {
        for (int i = 0; i < 13; i++)
        {
            int pid, xstatus;
            pid = fork();
            if (pid == 0)
            {
                if (strlen(benchmarks[i][1]) == 0) // To handle benchmarks without additional arguments
                {
                    char *temp[] = {benchmarks[i][0], 0};
                    fprintf(stderr, "Start %s\n", benchmarks[i][0]);
                    exec(benchmarks[i][0], temp);
                }
                else if (strlen(benchmarks[i][2]) == 0) // To handle benchmarks without additional arguments
                {
                    char *temp[] = {benchmarks[i][0], benchmarks[i][1], 0};
                    fprintf(stderr, "Start %s %s\n", benchmarks[i][0], benchmarks[i][1]);
                    exec(benchmarks[i][0], temp);
                }
                else if (strlen(benchmarks[i][3]) == 0) // To handle benchmarks without additional arguments
                {
                    char *temp[] = {benchmarks[i][0], benchmarks[i][1], benchmarks[i][2], 0};
                    fprintf(stderr, "Start %s %s %s\n", benchmarks[i][0], benchmarks[i][1], benchmarks[i][2]);
                    exec(benchmarks[i][0], temp);
                }
                else
                {
                    char *temp[] = {benchmarks[i][0], benchmarks[i][1], benchmarks[i][2], benchmarks[i][3], 0};
                    fprintf(stderr, "Start %s %s %s %s\n", benchmarks[i][0], benchmarks[i][1], benchmarks[i][2], benchmarks[i][3]);
                    exec(benchmarks[i][0], temp);
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
    }

    exit(0);
}
