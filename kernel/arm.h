#define PGSIZE 4096

static inline uint64
r_sp()
{
  uint64 x;
  asm volatile("mov %0, sp" : "=r" (x) );
  return x;
}
