# Initial process execs /init.

#include "syscall.h"

.globl start

# exec(init, argv)
start:
    LDR     x0, =init
    LDR     x1, =argv
    MOV     x7, #SYS_exec
    SVC     0x00

exit:
    MOV     x7, #SYS_exit
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

