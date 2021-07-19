
.global trapret

.macro	exception_1_entry
	sub sp, sp, #0x20        /* reserverd for LR, SP, SPSR and ELR */
	stp	x28, x29, [sp, #-16]!
	stp	x26, x27, [sp, #-16]!
	stp	x24, x25, [sp, #-16]!
	stp	x22, x23, [sp, #-16]!
	stp	x20, x21, [sp, #-16]!
	stp	x18, x19, [sp, #-16]!
	stp	x16, x17, [sp, #-16]!
	stp	x14, x15, [sp, #-16]!
	stp	x12, x13, [sp, #-16]!
	stp	x10, x11, [sp, #-16]!
	stp	x8, x9, [sp, #-16]!
	stp	x6, x7, [sp, #-16]!
	stp	x4, x5, [sp, #-16]!
	stp	x2, x3, [sp, #-16]!
	stp	x0, x1, [sp, #-16]!
	add x21, sp, #0x110

	mrs	x22, elr_el1             /* ELR */
	mrs	x23, spsr_el1            /* SPSR */

	stp	x30, x21, [sp, #0xf0]    /* LR, SP */
	stp	x22, x23, [sp, #0x100]   /* ELR, SPSR */
.endm

.macro	exception_1_exit
	ldp	x22, x23, [sp, #0x100]   /* ELR, SPSR */
	ldp	x30, x28, [sp, #0xf0]    /* LR, SP */

	msr	elr_el1, x22             /* ELR */
	msr	spsr_el1, x23
	
	mov x29, sp
	mov sp, x28

	ldp	x0, x1, [x29], #16
	ldp	x2, x3, [x29], #16
	ldp	x4, x5, [x29], #16
	ldp	x6, x7, [x29], #16
	ldp	x8, x9, [x29], #16
	ldp	x10, x11, [x29], #16
	ldp	x12, x13, [x29], #16
	ldp	x14, x15, [x29], #16
	ldp	x16, x17, [x29], #16
	ldp	x18, x19, [x29], #16
	ldp	x20, x21, [x29], #16
	ldp	x22, x23, [x29], #16
	ldp	x24, x25, [x29], #16
	ldp	x26, x27, [x29], #16
	ldr x28, [x29], #8
	ldr x29, [x29]
	eret
.endm

.macro	exception_0_entry
	sub sp, sp, #0x20        /* reserverd for LR, SP, SPSR and ELR */
	stp	x28, x29, [sp, #-16]!
	stp	x26, x27, [sp, #-16]!
	stp	x24, x25, [sp, #-16]!
	stp	x22, x23, [sp, #-16]!
	stp	x20, x21, [sp, #-16]!
	stp	x18, x19, [sp, #-16]!
	stp	x16, x17, [sp, #-16]!
	stp	x14, x15, [sp, #-16]!
	stp	x12, x13, [sp, #-16]!
	stp	x10, x11, [sp, #-16]!
	stp	x8, x9, [sp, #-16]!
	stp	x6, x7, [sp, #-16]!
	stp	x4, x5, [sp, #-16]!
	stp	x2, x3, [sp, #-16]!
	stp	x0, x1, [sp, #-16]!

	mrs x21, sp_el0
	mrs	x22, elr_el1            /* ELR */
	mrs	x23, spsr_el1            /* SPSR */

	stp	x30, x21, [sp, #0xf0]    /* LR, SP */
	stp	x22, x23, [sp, #0x100]   /* ELR, SPSR */
.endm

.macro	exception_0_exit
	ldp	x22, x23, [sp, #0x100]   /* ELR, SPSR */
	ldp	x30, x28, [sp, #0xf0]    /* LR, SP */

	msr	elr_el1, x22             /* ELR */
	msr	spsr_el1, x23
	msr sp_el0, x28

	mov x29, sp
	add sp, sp, #0x110

	ldp	x0, x1, [x29], #16
	ldp	x2, x3, [x29], #16
	ldp	x4, x5, [x29], #16
	ldp	x6, x7, [x29], #16
	ldp	x8, x9, [x29], #16
	ldp	x10, x11, [x29], #16
	ldp	x12, x13, [x29], #16
	ldp	x14, x15, [x29], #16
	ldp	x16, x17, [x29], #16
	ldp	x18, x19, [x29], #16
	ldp	x20, x21, [x29], #16
	ldp	x22, x23, [x29], #16
	ldp	x24, x25, [x29], #16
	ldp	x26, x27, [x29], #16
	ldr x28, [x29], #8
	ldr x29, [x29]
	eret
.endm

trapret:
	exception_0_exit

/* Exception vectors */

	.align	12
	.global	vectors
vectors:
	/* Current EL with SP0 */
	.align	7
	b	_el_current_bad_sync
	.align	7
	b	_el_current_bad_irq
	.align	7
	b	_el_current_bad_fiq
	.align	7
	b	_el_current_bad_error

	/* Current EL with SPx */
	.align	7
	b	_el_current_sync
	.align	7
	b	_el_current_irq
	.align	7
	b	_el_current_fiq
	.align	7
	b	_el_current_error

	/* Lower EL using AArch64 */
	.align	7
	b	_el_lower_sync
	.align	7
	b	_el_lower_irq
	.align	7
	b	_el_lower_fiq
	.align	7
	b	_el_lower_error

	.align	6
_el_current_bad_sync:
	exception_1_entry
	mov	x0, sp
	mov	x1, #1
	mrs	x2, esr_el1
	// bal	bad_handler
	b	.

	.align	6
_el_current_bad_irq:
	exception_1_entry
	mov	x0, sp
	mov	x1, #2
	mrs	x2, esr_el1
	// bal	bad_handler
	b	.

	.align	6
_el_current_bad_fiq:
	exception_1_entry
	mov	x0, sp
	mov	x1, #3
	mrs	x2, esr_el1
	// bal	bad_handler
	b	.

	.align	6
_el_current_bad_error:
	exception_1_entry
	mov	x0, sp
	mov	x1, #4
	mrs	x2, esr_el1
	// bal	bad_handler
	b	.


	.align	6
_el_current_sync:
	exception_1_entry
	mrs	x2, esr_el1
	lsr	x24, x2, #26
	cmp	x24, #0x25
	b.eq	el1_da
	cmp	x24, #0x21
	b.eq	el1_ia
	b	el1_default

el1_da:
	mov	x0, sp
	mov	x1, #1
	// bal	dabort_handler
	b	.

el1_ia:
	mov	x0, sp
	mov	x1, #1
	// bal	iabort_handler
	b	.

el1_default:
	mov	x0, sp
	mov	x1, #1
	// bal	default_handler
	b	.

	.align	6
_el_current_irq:
	exception_1_entry
	mov	x0, sp
	mov	x1, #1
	mrs	x2, esr_el1
	// bal	irq_handler
	exception_1_exit

	.align	6
_el_current_fiq:
	exception_1_entry
	mov	x0, sp
	mov	x1, #1
	mrs	x2, esr_el1
	// bal	fiq_handler
	b	.

	.align	6
_el_current_error:
	exception_1_entry
	mov	x0, sp
	mov	x1, #1
	mrs	x2, esr_el1
	// bal	error_handler
	b	.


	.align	6
_el_lower_sync:
	exception_0_entry
	mrs	x2, esr_el1
	lsr	x24, x2, #26
	cmp	x24, #0x15
	b.eq	el0_svc
	cmp	x24, #0x24
	b.eq	el0_da
	cmp	x24, #0x20
	b.eq	el0_ia
	cmp	x24, #0x00
	b.eq	el0_undef
	b	el0_default

el0_svc:
	mov	x0, sp
	mov	x1, #0
	// bal	swi_handler
	exception_0_exit

el0_da:
	mov	x0, sp
	mov	x1, #0
	// bal	dabort_handler
	b	.

el0_ia:
	mov	x0, sp
	mov	x1, #0
	// bal	iabort_handler
	b	.

el0_undef:
	mov	x0, sp
	mov	x1, #0
	// bal	und_handler
	b	.

el0_default:
	mov	x0, sp
	mov	x1, #0
	// bal	default_handler
	b	.

	.align	6
_el_lower_irq:
	exception_0_entry
	mov	x0, sp
	mov	x1, #0
	mrs	x2, esr_el1
	// bal	irq_handler
	exception_0_exit

	.align	6
_el_lower_fiq:
	exception_0_entry
	mov	x0, sp
	mov	x1, #0
	mrs	x2, esr_el1
	// bal	fiq_handler
	b	.

	.align	6
_el_lower_error:
	exception_0_entry
	mov	x0, sp
	mov	x1, #0
	mrs	x2, esr_el1
	// bal	error_handler
	b	.

