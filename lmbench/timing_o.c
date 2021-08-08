// #include <stdio.h>
// #include "bench.h"
#include "user/user.h"

int
main()
{
	putenv("LOOP_O=0.0");
	printf("%lu\n", (unsigned long)t_overhead());
	return (0);
}
