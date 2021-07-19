# Initial process execs /init.

#include "syscall.h"

.globl start

# exec(init, argv)
start:
    LDR     x1, =init
    LDR     x2, =argv
    MOV     x0, #SYS_exec
    SVC     0x00

exit:
    MOV     x0, #SYS_exit
    SVC     0x00
    B       exit

# char init[] = "/init\0";
init:
    .string "/init\0"

# char *argv[] = { init, 0 };
.p2align 4
argv:
    .word init
    .word 0
    .word 0
    .word 0

