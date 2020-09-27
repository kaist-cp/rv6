// init: The initial user-level program

#include "kernel/types.h"
#include "kernel/stat.h"
#include "user/user.h"
#include "kernel/fcntl.h"

#ifdef USERTEST
char *argv[] = { "usertests", 0 };
#else
char *argv[] = { "sh", 0 };
#endif

int
main(void)
{
  int pid, wpid, xstate;

  if(open("console", O_RDWR) < 0){
    mknod("console", 1, 1);
    open("console", O_RDWR);
  }
  dup(0);  // stdout
  dup(0);  // stderr

  for(;;){
    printf("init: starting %s\n", argv[0]);
    pid = fork();
    if(pid < 0){
      printf("init: fork failed\n");
      exit(1);
    }
    if(pid == 0){
      exec(argv[0], argv);
      printf("init: exec %s failed\n", argv[0]);
      exit(1);
    }
    while((wpid=wait(&xstate)) >= 0 && wpid != pid){
      //printf("zombie!\n");
    }
#ifdef USERTEST
    poweroff(xstate);
#endif
  }
}
