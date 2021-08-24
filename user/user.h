#include <kernel/types.h>

struct stat;
struct rtcdate;

// system calls
int fork(void);
int exit(int) __attribute__((noreturn));
int wait(int*);
int pipe(int*);
int write(int, const void*, int);
int read(int, void*, int);
int close(int);
int kill(int);
int exec(char*, char**);
int open(const char*, int);
int mknod(const char*, short, short);
int unlink(const char*);
int fstat(int fd, struct stat*);
int link(const char*, const char*);
int mkdir(const char*);
int chdir(const char*);
int dup(int);
int getpid(void);
char* sbrk(int);
int sleep(int);
int uptime(void);
int poweroff(int) __attribute__((noreturn));

// newly added system calls
int select(int nfds, fd_set *restrict readfds,
            fd_set *restrict writefds, fd_set *restrict exceptfds,
            int timeout);
int getpagesize(void);
int waitpid(int pid, int *stat_loc, int options);
int getppid(void);
off_t lseek(int fildes, off_t offset, int whence);
int uptime_as_micro();
int gettimeofday(struct timeval *__restrict__ tp,
                struct timezone *__restrict__ tzp);
int clock(unsigned long*);

// ulib.c
int stat(const char*, struct stat*);
char* strcpy(char*, const char*);
void *memmove(void*, const void*, int);
char* strchr(const char*, char c);
int strcmp(const char*, const char*);
void fprintf(int, const char*, ...);
void printf(const char*, ...);
char* gets(char*, int max);
uint strlen(const char*);
void* memset(void*, int, uint);
void* malloc(uint);
void free(void*);
int atoi(const char*);
int memcmp(const void *, const void *, uint);
void *memcpy(void *, const void *, uint);

// newly added ulibs
int posix_select(int nfds, fd_set *restrict readfds,
            fd_set *restrict writefds, fd_set *restrict exceptfds,
            struct timeval* timeout);
int posix_kill(pid_t pid, int sig);
int posix_open3(const char *pathname, int flags, mode_t mode);
void usleep(unsigned long useconds);
int posix_exit(int);
int posix_mkdir(const char *pathname, mode_t mode);

// <signal.h>
typedef void (*sighandler_t)(int);
sighandler_t signal(int signum, sighandler_t handler);
unsigned int alarm(unsigned int seconds);

// <unistd.h>
int fsync(int fildes);
char* getenv(const char *varname);

// <stdio.h>
void perror(const char *s);
int sscanf(const char *buffer, const char *format, ...);
int sprintf(char *buffer, const char *format_string, ...);

// <stdlib.h>
double atof(const char *string);
void *valloc(size_t size);


// #ifndef SRAND_DEF
// #define SRAND_DEF
// void srand(unsigned int seed);
// #endif

void qsort(void *base, size_t num, size_t width,
    int(*compare)(const void *element1, const void *element2));
void *realloc(void *ptr, size_t size);

double strtod(const char * string, char **endPtr);
long int strtol(const char *nptr, char **endptr, int base);

// <assert.h>
void assert(int expression);

// <ctype.h>
int isdigit(int arg);


int creat(const char *path, mode_t mode);
char *strdup(const char *s);
int execlp(const char *file, const char *arg, .../*, (char *) NULL */);
int execve(const char *pathname, char *const argv[],
            char *const envp[]);
int rmdir(const char *pathname);
char *tempnam(const char *dir, const char *pfx);
int fflush(int stream);
int putenv(char *string);

// <time.h>
int setitimer(int which, const struct itimerval *new_value,
              struct itimerval *old_value);
// #ifndef RAND_DEFINED
// #define RAND_DEFINED
// int rand(void);
// #endif

// <signal.h>
int sigaction(int signum, const struct sigaction *restrict act,
                     struct sigaction *restrict oldact);
int sigemptyset(sigset_t *set);



# define __FDS_BITS(set) ((set)->__fds_bits)
#define FD_ZERO(set) \
  do {									      \
    unsigned int __i;							      \
    fd_set *__arr = (set);						      \
    for (__i = 0; __i < sizeof (fd_set) / sizeof (__fd_mask); ++__i)	      \
      __FDS_BITS (__arr)[__i] = 0;					      \
  } while (0)

#define __NFDBITS	(8 * (int) sizeof (__fd_mask))
#define	__FD_ELT(d)	((d) / __NFDBITS)
#define	__FD_MASK(d)	((__fd_mask) (1UL << ((d) % __NFDBITS)))

#define FD_SET(d, set) \
  ((void) (__FDS_BITS (set)[__FD_ELT (d)] |= __FD_MASK (d)))

#define FD_ISSET(d, s) \
  ((__FDS_BITS (s)[__FD_ELT (d)] & __FD_MASK (d)) != 0)

#define L_tmpnam 20
