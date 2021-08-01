


.section trampsec
.globl trampoline
trampoline:

.macro	exception_1_entry
	sub	sp, sp, #272
	stp	x0, x1, [sp, #16 * 0]
	stp	x2, x3, [sp, #16 * 1]
	stp	x4, x5, [sp, #16 * 2]
	stp	x6, x7, [sp, #16 * 3]
	stp	x8, x9, [sp, #16 * 4]
	stp	x10, x11, [sp, #16 * 5]
	stp	x12, x13, [sp, #16 * 6]
	stp	x14, x15, [sp, #16 * 7]
	stp	x16, x17, [sp, #16 * 8]
	stp	x18, x19, [sp, #16 * 9]
	stp	x20, x21, [sp, #16 * 10]
	stp	x22, x23, [sp, #16 * 11]
	stp	x24, x25, [sp, #16 * 12]
	stp	x26, x27, [sp, #16 * 13]
	stp	x28, x29, [sp, #16 * 14]

	add	x21, sp, #272

	mrs	x22, elr_el1
	mrs	x23, spsr_el1

	stp	x30, x21, [sp, #16 * 15] 
	stp	x22, x23, [sp, #16 * 16]
.endm

.macro	exception_1_exit
	ldp	x22, x23, [sp, #16 * 16]
	ldp	x30, x21, [sp, #16 * 15]

	msr	elr_el1, x22
	msr	spsr_el1, x23

	ldp	x0, x1, [sp, #16 * 0]
	ldp	x2, x3, [sp, #16 * 1]
	ldp	x4, x5, [sp, #16 * 2]
	ldp	x6, x7, [sp, #16 * 3]
	ldp	x8, x9, [sp, #16 * 4]
	ldp	x10, x11, [sp, #16 * 5]
	ldp	x12, x13, [sp, #16 * 6]
	ldp	x14, x15, [sp, #16 * 7]
	ldp	x16, x17, [sp, #16 * 8]
	ldp	x18, x19, [sp, #16 * 9]
	ldp	x20, x21, [sp, #16 * 10]
	ldp	x22, x23, [sp, #16 * 11]
	ldp	x24, x25, [sp, #16 * 12]
	ldp	x26, x27, [sp, #16 * 13]
	ldp	x28, x29, [sp, #16 * 14]
	add	sp, sp, #272		
	eret
.endm

.macro	exception_0_entry

        /* assign TRAPFRAME address (fixed) to x0
        TODO: replace this with macro? */
        str     x0, [sp], #-8
        movz    x0, #0xe000
        movk    x0, #0xffff, lsl #16
        movk    x0, #0x3f, lsl #32

        // save the user registers in TRAPFRAME
        stp     x1, x2, [x0, #40]
        stp     x3, x4,  [x0, #56]
        stp     x5, x6,  [x0, #72]
        stp     x7, x8,  [x0, #88]
        stp     x7, x8,  [x0, #104]
        stp     x9, x10,  [x0, #120]
        stp     x11, x12,  [x0, #136]
        stp     x13, x14,  [x0, #152]
        stp     x15, x16,  [x0, #168]
        stp     x17, x18,  [x0, #184]
        stp     x19, x20,  [x0, #200]
        stp     x21, x22,  [x0, #216]
        stp     x23, x24,  [x0, #232]
        stp     x25, x26,  [x0, #248]
        stp     x27, x28,  [x0, #264]
        stp     x29, x30,  [x0, #280]
	
        mrs     x21, sp_el0             /* SP: user stack */
	mrs	x22, elr_el1            /* ELR (user mode pc) */
	mrs	x23, spsr_el1           /* SPSR */

	stp	x21, x22, [x0, #16]     /* SP, ELR */
	str	x23, [x0, #8]           /* SPSR */

        ldr     x24, [sp], #-8
        str     x24, [x0, #32]               /* store x0 */
        
        # restore kernel page table from p->trapframe->kernel_satp
        ldr     x24, [x0]
        msr     ttbr0_el1, x24
.endm

/* Exception vectors */

	.align	12
	.globl	vectors
vectors:
	/* Current EL with SP0 */
	.align	7
	b	cur_el_sp0_sync
	.align	7
	b	cur_el_sp0_irq
	.align	7
	b	cur_el_sp0_fiq
	.align	7
	b	cur_el_sp0_error

	/* Current EL with SPx */
	.align	7
	b	cur_el_spx_sync
	.align	7
	b	cur_el_spx_irq
	.align	7
	b	cur_el_spx_fiq
	.align	7
	b	cur_el_spx_error

	/* Lower EL using AArch64 */
	.align	7
	b	lower_el_aarch64_sync
	.align	7
	b	lower_el_aarch64_irq
	.align	7
	b	lower_el_aarch64_fiq
	.align	7
	b	lower_el_aarch64_error

.align 4
.globl userret
userret:
        # userret(TRAPFRAME, pagetable)
        # switch from kernel to user.
        # usertrapret() calls here.
        # a0: TRAPFRAME, in user page table.
        # a1: user page table, for satp.

        # switch to the user page table.
        msr ttbr0_el1, x1
        isb

        # restore ELR, SPSR, LR, SP
	ldp	x21, x22, [x0, #8]       /* SPSR, SP */ 
	ldr	x23, [x0, #24]           /* ELR */

	msr	spsr_el1, x21            /* SPSR */
	msr     sp_el0, x22              /* SP */
	msr	elr_el1, x23             /* ELR */

        # restore all but a0 from TRAPFRAME
	ldp	x1, x2, [x0, #40]
	ldp	x3, x4, [x0, #56]
	ldp	x5, x6, [x0, #72]
	ldp	x7, x8, [x0, #88]
	ldp	x9, x10, [x0, #104]
	ldp	x11, x12, [x0, #120]
	ldp	x13, x14, [x0, #136]
	ldp	x15, x16, [x0, #152]
	ldp	x17, x18, [x0, #168]
	ldp	x19, x20, [x0, #184]
	ldp	x21, x22, [x0, #200]
	ldp	x23, x24, [x0, #216]
	ldp	x25, x26, [x0, #232]
	ldp	x27, x28, [x0, #248]
	ldp	x29, x30, [x0, #264]
        ldr     x0, [x0, #32]
	eret



	.align	6
cur_el_sp0_sync:
	exception_1_entry
	mov	x0, #0
	bl	cur_el_sp0_handler
	b	.

	.align	6
cur_el_sp0_irq:
	exception_1_entry
	mov	x0, #1
	bl	cur_el_sp0_handler
	b	.

	.align	6
cur_el_sp0_fiq:
	exception_1_entry
	mov	x0, #2
	bl	cur_el_sp0_handler
	b	.

	.align	6
cur_el_sp0_error:
	exception_1_entry
	mov	x0, #3
	bl	cur_el_sp0_handler
	b	.

	.align	6
cur_el_spx_sync:
	exception_1_entry
        mov	x0, #0
        bl cur_el_sp1_handler
        exception_1_exit

	.align	6
cur_el_spx_irq:
	exception_1_entry
	mov	x0, #1
        bl cur_el_sp1_handler
	exception_1_exit

	.align	6
cur_el_spx_fiq:
        exception_1_entry
	mov	x0, #2
        bl cur_el_sp1_handler
	b	.

	.align	6
cur_el_spx_error:
        exception_1_entry
	mov	x0, #3
        bl cur_el_sp1_handler
	b	.

	.align	6
lower_el_aarch64_sync:
	exception_0_entry
        mov	x0, #0
	bl	usertrap

	.align	6
lower_el_aarch64_irq:
        exception_0_entry
        mov	x0, #1
	bl	usertrap

	.align	6
lower_el_aarch64_fiq:
        exception_0_entry
        mov	x0, #2
	bl	usertrap
        b       .

	.align	6
lower_el_aarch64_error:
        exception_0_entry
        mov	x0, #3
	bl	usertrap
	b	.

