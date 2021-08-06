
.equ _core_id_mask, 0b11

        # qemu -kernel loads the kernel at 0x40010000
        # and causes each CPU to jump there.
        # kernel.ld causes the following code to
        # be placed at 0x40010000.
.global _entry
.section .text
_entry:
        # initialize stack pointers for svc modes
	mov     x0, #1     // select SP_EL1
	msr     spsel, x0
	isb

	# set up a stack for C.
        # stack0 is declared in start.c,
        # with a 4096-byte stack per CPU.
        # sp = stack0 + (coreid * 4096)
        adrp x0, stack0
        mov sp, x0
        mov x0, #4096
        mrs x1, MPIDR_EL1 
        and x1, x1, _core_id_mask
        add x1, x1, #1
        mul x0, x0, x1
        add sp, sp, x0
	# jump to start() in start.c
        b start
spin:
        b spin
