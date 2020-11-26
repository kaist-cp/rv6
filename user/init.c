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
  // https://github.com/kaist-cp/rv6/commit/d12c1db8d9d7a7e5632e51ae712123d868087fe4
  // Add xstate to immediately run usertests and poweroff.
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

    for(;;){
      // this call to wait() returns if the shell exits,
      // or if a parentless process exits.
      wpid = wait(&xstate);
      if(wpid == pid){
        // the shell exited; restart it.
        break;
      } else if(wpid < 0){
        printf("init: wait returned an error\n");
        exit(1);
      } else {
        // it was a parentless process; do nothing.
      }
    }
#ifdef USERTEST
    poweroff(xstate);
#endif
  }
}
