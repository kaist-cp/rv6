# Context switch
#
#   void swtch(struct context **old, struct context *new);
#
# Save current register context in old
# and then load register context from new.
# The stack is as r4_svc-r12_svc, lr_svc, sp_usr, lr_usr, and pc_usr
# This file is from https://github.com/sudharson14/xv6-OS-for-arm-v8/blob/master/xv6-armv8/swtch.S,
# and modified according to rv6's design.

.globl swtch
swtch:
    stp     x4, x5, [x0]
	stp	x6, x7, [x0, #16]!
	stp	x8, x9, [x0, #16]!
	stp	x10, x11, [x0, #16]!
	stp	x12, x13, [x0, #16]!
	stp	x14, x15, [x0, #16]!
	stp	x16, x17, [x0, #16]!
	stp	x18, x19, [x0, #16]!
	stp	x20, x21, [x0, #16]!
	stp	x22, x23, [x0, #16]!
	stp	x24, x25, [x0, #16]!
	stp	x26, x27, [x0, #16]!
	stp	x28, x29, [x0, #16]!
    mov     x2, sp
	stp	lr, x2, [x0, #16]!

	# switch the stack
	//mrs	x20, sp_el0
	# mov	x21, sp
	# str	x21, [x0]
	# mov	sp, x1

	# load the new registers. pc_usr is not restored here because
	# LDMFD^ will switch mode if pc_usr is loaded. We just simply
	# pop it out as pc_usr is saved on the stack, and will be loaded
	# when we return from kernel to user space (swi or interrupt return)
        
	ldp	x4, x5, [x1]
	ldp	x6, x7, [x1, #16]!
	ldp	x8, x9, [x1, #16]!
	ldp	x10, x11, [x1, #16]!
	ldp	x12, x13, [x1, #16]!
	ldp	x14, x15, [x1, #16]!
	ldp	x16, x17, [x1, #16]!
	ldp	x18, x19, [x1, #16]!
	ldp	x20, x21, [x1, #16]!
	ldp	x22, x23, [x1, #16]!
	ldp	x24, x25, [x1, #16]!
	ldp	x26, x27, [x1, #16]!
	ldp	x28, x29, [x1, #16]!
	ldp	lr, x2, [x1, #16]!
    mov     sp, x2

	# return to the caller
	br	x30
