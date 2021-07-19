# Context switch
#
#   void swtch(struct context **old, struct context *new);
#
# Save current register context in old
# and then load register context from new.
# The stack is as r4_svc-r12_svc, lr_svc, sp_usr, lr_usr, and pc_usr
.global swtch

swtch:
	stp	x29, x30, [sp, #-16]!
	stp	x27, x28, [sp, #-16]!
	stp	x25, x26, [sp, #-16]!
	stp	x23, x24, [sp, #-16]!
	stp	x21, x22, [sp, #-16]!
	stp	x19, x20, [sp, #-16]!
	stp	x17, x18, [sp, #-16]!
	stp	x15, x16, [sp, #-16]!
	stp	x13, x14, [sp, #-16]!
	stp	x11, x12, [sp, #-16]!
	stp	x9, x10, [sp, #-16]!
	stp	x7, x8, [sp, #-16]!
	stp	x5, x6, [sp, #-16]!
	str	x4, [sp, #-8]!

	# switch the stack
	//mrs	x20, sp_el0
	mov	x21, sp
	str	x21, [x0]
	mov	sp, x1

	# load the new registers. pc_usr is not restored here because
	# LDMFD^ will switch mode if pc_usr is loaded. We just simply
	# pop it out as pc_usr is saved on the stack, and will be loaded
	# when we return from kernel to user space (swi or interrupt return)

	ldr	x4, [sp], #8
	ldp	x5, x6, [sp], #16
	ldp	x7, x8, [sp], #16
	ldp	x9, x10, [sp], #16
	ldp	x11, x12, [sp], #16
	ldp	x13, x14, [sp], #16
	ldp	x15, x16, [sp], #16
	ldp	x17, x18, [sp], #16
	ldp	x19, x20, [sp], #16
	ldp	x21, x22, [sp], #16
	ldp	x23, x24, [sp], #16
	ldp	x25, x26, [sp], #16
	ldp	x27, x28, [sp], #16
	ldp	x29, x30, [sp], #16

	# return to the caller
	br	x30


