#include "kernel/types.h"
#include "kernel/stat.h"
#include "kernel/fcntl.h"
#include "user/user.h"

#define MICROSECS_PER_TICK 100000

char*
strcpy(char *s, const char *t)
{
  char *os;

  os = s;
  while((*s++ = *t++) != 0)
    ;
  return os;
}

int
strcmp(const char *p, const char *q)
{
  while(*p && *p == *q)
    p++, q++;
  return (uchar)*p - (uchar)*q;
}

uint
strlen(const char *s)
{
  int n;

  for(n = 0; s[n]; n++)
    ;
  return n;
}

void*
memset(void *dst, int c, uint n)
{
  char *cdst = (char *) dst;
  int i;
  for(i = 0; i < n; i++){
    cdst[i] = c;
  }
  return dst;
}

char*
strchr(const char *s, char c)
{
  for(; *s; s++)
    if(*s == c)
      return (char*)s;
  return 0;
}

char*
gets(char *buf, int max)
{
  int i, cc;
  char c;

  for(i=0; i+1 < max; ){
    cc = read(0, &c, 1);
    if(cc < 1)
      break;
    buf[i++] = c;
    if(c == '\n' || c == '\r')
      break;
  }
  buf[i] = '\0';
  return buf;
}

int
stat(const char *n, struct stat *st)
{
  int fd;
  int r;

  fd = open(n, O_RDONLY);
  if(fd < 0)
    return -1;
  r = fstat(fd, st);
  close(fd);
  return r;
}

int
atoi(const char *s)
{
  int n;

  n = 0;
  while('0' <= *s && *s <= '9')
    n = n*10 + *s++ - '0';
  return n;
}

void*
memmove(void *vdst, const void *vsrc, int n)
{
  char *dst;
  const char *src;

  dst = vdst;
  src = vsrc;
  if (src > dst) {
    while(n-- > 0)
      *dst++ = *src++;
  } else {
    dst += n;
    src += n;
    while(n-- > 0)
      *--dst = *--src;
  }
  return vdst;
}

int
memcmp(const void *s1, const void *s2, uint n)
{
  const char *p1 = s1, *p2 = s2;
  while (n-- > 0) {
    if (*p1 != *p2) {
      return *p1 - *p2;
    }
    p1++;
    p2++;
  }
  return 0;
}

void *
memcpy(void *dst, const void *src, uint n)
{
  return memmove(dst, src, n);
}

void
bzero (void *to, size_t count)
{
  memset (to, 0, count);
}

void
bcopy(const void *src, void *dest, size_t n)
{
  memcpy(dest, src, n);
}



// ignore all signal function calls
void (*signal(int sig, void (*func)(int)))(int)
{
  return 0;
};

int
isdigit(int arg)
{
  return arg >= '0' && arg <= '9';
}

#define TOLOWER(c) (c >= 'A' && c <= 'Z' ? c - 'a' + 'A' : c)

/* Compare S1 and S2, ignoring case.  */
int
strcasecmp(const char *string1, const char *string2)
{
  const unsigned char *p1 = (const unsigned char *) string1;
  const unsigned char *p2 = (const unsigned char *) string2;
  int result;

  if (p1 == p2)
    return 0;

  while ((result = TOLOWER (*p1) - TOLOWER (*p2++)) == 0)
    if (*p1++ == '\0')
      break;

  return result;
}


/* Compare no more than N chars of S1 and S2, ignoring case.  */
int
strncasecmp(const char *string1, const char *string2, size_t n)
{
  const unsigned char *p1 = (const unsigned char *) string1;
  const unsigned char *p2 = (const unsigned char *) string2;
  int result;

  if (p1 == p2 || n == 0)
    return 0;

  while ((result = TOLOWER (*p1) - TOLOWER (*p2++)) == 0)
    if (*p1++ == '\0' || --n == 0)
      break;

  return result;
  return 0;
}


// TODO
int
putenv(char *varname)
{
  return 0;
}

double atof(const char *s)
{
  double a = 0.0;
  int e = 0;
  int c;
  while ((c = *s++) != '\0' && isdigit(c)) {
    a = a*10.0 + (c - '0');
  }
  if (c == '.') {
    while ((c = *s++) != '\0' && isdigit(c)) {
      a = a*10.0 + (c - '0');
      e = e-1;
    }
  }
  if (c == 'e' || c == 'E') {
    int sign = 1;
    int i = 0;
    c = *s++;
    if (c == '+')
      c = *s++;
    else if (c == '-') {
      c = *s++;
      sign = -1;
    }
    while (isdigit(c)) {
      i = i*10 + (c - '0');
      c = *s++;
    }
    e += i*sign;
  }
  while (e > 0) {
    a *= 10.0;
    e--;
  }
  while (e < 0) {
    a *= 0.1;
    e++;
  }
  return a;
}

// nothing to be done
int fsync(int fildes)
{
  return 0;
}

// bool_t
// pmap_set (ulong program, ulong version, int protocol, ushort port)
// {
//   return 0;
// }

// bool_t
// pmap_unset (ulong program, ulong version)
// {
//   return 0;
// }

// ushort pmap_getport (struct sockaddr_in *address, ulong program, ulong version, uint protocol)
// {
//   return 0;
// }

// TODO: find better way to convert uptime ticks to real time.
int gettimeofday(struct timeval *__restrict__ tp, 
                struct timezone *__restrict__ tzp)
{
  // assume 1tick = 100000 microsecs
  tp->tv_sec = (uptime() * MICROSECS_PER_TICK) / 1000000;
  tp->tv_usec = (uptime() * MICROSECS_PER_TICK) % 1000000;
  // fprintf(1, "now: %d sec, %d us\n", tp->tv_sec, tp->tv_usec);
  return 0;
}

int * __errno_location(void){
  return 0;
}

// void *mmap(void *addr, size_t length, int prot, int flags,
//           int fd, off_t offset)
// {
//   return 0;
// }

// int munmap(void *addr, size_t length)
// {
//   return 0;
// }

void usleep(unsigned long useconds) {
  sleep(useconds / MICROSECS_PER_TICK);
}

int creat(const char *path, mode_t mode){
  return open(path, O_CREATE | O_WRONLY | O_TRUNC);
}

int rmdir(const char *pathname) {
  return unlink(pathname);
}

int posix_select(int nfds, fd_set *restrict readfds,
            fd_set *restrict writefds, fd_set *restrict exceptfds,
            struct timeval* timeout)
{
  long ticks = (timeout->tv_sec * 1000000 + timeout->tv_usec) / MICROSECS_PER_TICK;
  return select(nfds, readfds, writefds, 0, ticks);
}

#define CHAR_BIT 8
/* Discontinue quicksort algorithm when partition gets below this size.
   This particular magic number was chosen to work best on a Sun 4/260. */
#define MAX_THRESH 4
#define	STACK_NOT_EMPTY	(stack < top)
#define STACK_SIZE	(CHAR_BIT * sizeof (size_t))
#define PUSH(low, high)	((void) ((top->lo = (low)), (top->hi = (high)), ++top))
#define	POP(low, high)	((void) (--top, (low = top->lo), (high = top->hi)))
#define SWAP(a, b, size)						      \
  do									      \
    {									      \
      size_t __size = (size);						      \
      char *__a = (a), *__b = (b);					      \
      do								      \
	{								      \
	  char __tmp = *__a;						      \
	  *__a++ = *__b;						      \
	  *__b++ = __tmp;						      \
	} while (--__size > 0);						      \
    } while (0)

typedef struct
  {
    char *lo;
    char *hi;
  } stack_node;

void qsort(void *base, size_t total_elems, size_t size,
          int (*cmp)(const void *, const void *))
{
    char *base_ptr = (char *) base;

  const size_t max_thresh = MAX_THRESH * size;

  if (total_elems == 0)
    /* Avoid lossage with unsigned arithmetic below.  */
    return;

  if (total_elems > MAX_THRESH)
    {
      char *lo = base_ptr;
      char *hi = &lo[size * (total_elems - 1)];
      stack_node stack[STACK_SIZE];
      stack_node *top = stack;

      PUSH (NULL, NULL);

      while (STACK_NOT_EMPTY)
        {
          char *left_ptr;
          char *right_ptr;

	  /* Select median value from among LO, MID, and HI. Rearrange
	     LO and HI so the three values are sorted. This lowers the
	     probability of picking a pathological pivot value and
	     skips a comparison for both the LEFT_PTR and RIGHT_PTR in
	     the while loops. */

	  char *mid = lo + size * ((hi - lo) / size >> 1);

	  if ((*cmp) ((void *) mid, (void *) lo) < 0)
	    SWAP (mid, lo, size);
	  if ((*cmp) ((void *) hi, (void *) mid) < 0)
	    SWAP (mid, hi, size);
	  else
	    goto jump_over;
	  if ((*cmp) ((void *) mid, (void *) lo) < 0)
	    SWAP (mid, lo, size);
	jump_over:;

	  left_ptr  = lo + size;
	  right_ptr = hi - size;

	  /* Here's the famous ``collapse the walls'' section of quicksort.
	     Gotta like those tight inner loops!  They are the main reason
	     that this algorithm runs much faster than others. */
	  do
	    {
	      while ((*cmp) ((void *) left_ptr, (void *) mid) < 0)
		left_ptr += size;

	      while ((*cmp) ((void *) mid, (void *) right_ptr) < 0)
		right_ptr -= size;

	      if (left_ptr < right_ptr)
		{
		  SWAP (left_ptr, right_ptr, size);
		  if (mid == left_ptr)
		    mid = right_ptr;
		  else if (mid == right_ptr)
		    mid = left_ptr;
		  left_ptr += size;
		  right_ptr -= size;
		}
	      else if (left_ptr == right_ptr)
		{
		  left_ptr += size;
		  right_ptr -= size;
		  break;
		}
	    }
	  while (left_ptr <= right_ptr);

          /* Set up pointers for next iteration.  First determine whether
             left and right partitions are below the threshold size.  If so,
             ignore one or both.  Otherwise, push the larger partition's
             bounds on the stack and continue sorting the smaller one. */

          if ((size_t) (right_ptr - lo) <= max_thresh)
            {
              if ((size_t) (hi - left_ptr) <= max_thresh)
		/* Ignore both small partitions. */
                POP (lo, hi);
              else
		/* Ignore small left partition. */
                lo = left_ptr;
            }
          else if ((size_t) (hi - left_ptr) <= max_thresh)
	    /* Ignore small right partition. */
            hi = right_ptr;
          else if ((right_ptr - lo) > (hi - left_ptr))
            {
	      /* Push larger left partition indices. */
              PUSH (lo, right_ptr);
              lo = left_ptr;
            }
          else
            {
	      /* Push larger right partition indices. */
              PUSH (left_ptr, hi);
              hi = right_ptr;
            }
        }
    }

  /* Once the BASE_PTR array is partially sorted by quicksort the rest
     is completely sorted using insertion sort, since this is efficient
     for partitions below MAX_THRESH size. BASE_PTR points to the beginning
     of the array to sort, and END_PTR points at the very last element in
     the array (*not* one beyond it!). */

#define min(x, y) ((x) < (y) ? (x) : (y))

  {
    char *const end_ptr = &base_ptr[size * (total_elems - 1)];
    char *tmp_ptr = base_ptr;
    char *thresh = min(end_ptr, base_ptr + max_thresh);
    char *run_ptr;

    /* Find smallest element in first threshold and place it at the
       array's beginning.  This is the smallest array element,
       and the operation speeds up insertion sort's inner loop. */

    for (run_ptr = tmp_ptr + size; run_ptr <= thresh; run_ptr += size)
      if ((*cmp) ((void *) run_ptr, (void *) tmp_ptr) < 0)
        tmp_ptr = run_ptr;

    if (tmp_ptr != base_ptr)
      SWAP (tmp_ptr, base_ptr, size);

    /* Insertion sort, running from left-hand-side up to right-hand-side.  */

    run_ptr = base_ptr + size;
    while ((run_ptr += size) <= end_ptr)
      {
	tmp_ptr = run_ptr - size;
	while ((*cmp) ((void *) run_ptr, (void *) tmp_ptr) < 0)
	  tmp_ptr -= size;

	tmp_ptr += size;
        if (tmp_ptr != run_ptr)
          {
            char *trav;

	    trav = run_ptr + size;
	    while (--trav >= run_ptr)
              {
                char c = *trav;
                char *hi, *lo;

                for (hi = lo = trav; (lo -= size) >= tmp_ptr; hi = lo)
                  *hi = *lo;
                *hi = c;
              }
          }
      }
  }
}

// simple implementation of newton's algorithm
double sqrt(double x)
{
  int i;
  double z = 1.0;
  for(i = 1; i <= 10; i++){
    z -= (z*z - x) / (2*z); // MAGIC LINE!!
  }
  return z;
}

// don't implement now
int fflush(int stream) {
  return 0;
}

// TODO
int
execve(const char *pathname, char *const argv[], char *const envp[])
{
  return exec((char*)pathname, (char**)argv);
}

// TODO
// int
// execlp(const char *file, const char *arg, .../*, (char *) NULL */)
// {
//   return 0;
//   // return exec((char*)file, arg);
// }

// nothing to do
unsigned int
alarm(unsigned int seconds)
{
  return 0;
}

// nothing to do
int
setitimer(int which, const struct itimerval *new_value,
              struct itimerval *old_value)
{
  return 0;
}

int
sigaction(int signum, const struct sigaction *restrict act,
                     struct sigaction *restrict oldact)
{
  return 0;
}

int
sigemptyset(sigset_t *set)
{
  return 0;
}

char*
strerror(int errno)
{
  return NULL;  
}

int
execvp(const char * file, char * const argv[])
{
  return exec((char*)file, (char**)argv);
  // return execlp(file, argv);
}

int
posix_kill(pid_t pid, int sig)
{
  return kill(pid);
}

int
posix_open3(const char *pathname, int flags, mode_t mode)
{
  // Note: mode is ignored
  return open(pathname, flags);
}

int
posix_exit(int i)
{
  return exit(i);  
}

int
posix_mkdir(const char *pathname, mode_t mode)
{
  return mkdir(pathname);
}
