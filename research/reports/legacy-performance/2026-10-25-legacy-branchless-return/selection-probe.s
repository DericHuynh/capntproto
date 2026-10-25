	.file	"selection_probe.6863bc1677bde6c4-cgu.0"
	.section	.text.baseline,"ax",@progbits
	.globl	baseline
	.p2align	4
	.type	baseline,@function
baseline:
	.cfi_startproc
	movq	(%rdi), %rax
	testq	%rax, %rax
	je	.LBB0_3
	decq	%rax
	movq	%rax, (%rdi)
	je	.LBB0_4
	xorl	%eax, %eax
	retq
.LBB0_4:
	movzbl	(%rsi), %eax
	movb	$1, (%rsi)
	xorb	$1, %al
	retq
.LBB0_3:
	pushq	%rax
	.cfi_def_cfa_offset 16
	leaq	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.0(%rip), %rdi
	leaq	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.2(%rip), %rdx
	movl	$22, %esi
	callq	*_RNvNtCscI6d9CVNmLh_4core6option13expect_failed@GOTPCREL(%rip)
.Lfunc_end0:
	.size	baseline, .Lfunc_end0-baseline
	.cfi_endproc

	.section	.text.candidate,"ax",@progbits
	.globl	candidate
	.p2align	4
	.type	candidate,@function
candidate:
	.cfi_startproc
	movq	(%rdi), %rax
	testq	%rax, %rax
	je	.LBB1_2
	decq	%rax
	movq	%rax, (%rdi)
	sete	%cl
	movzbl	(%rsi), %eax
	movl	%ecx, %edx
	orb	%al, %dl
	movb	%dl, (%rsi)
	xorb	$1, %al
	andb	%cl, %al
	retq
.LBB1_2:
	pushq	%rax
	.cfi_def_cfa_offset 16
	leaq	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.0(%rip), %rdi
	leaq	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.3(%rip), %rdx
	movl	$22, %esi
	callq	*_RNvNtCscI6d9CVNmLh_4core6option13expect_failed@GOTPCREL(%rip)
.Lfunc_end1:
	.size	candidate, .Lfunc_end1-candidate
	.cfi_endproc

	.type	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.0,@object
	.section	.rodata..Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.0,"a",@progbits
.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.0:
	.ascii	"return guard underflow"
	.size	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.0, 22

	.type	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.1,@object
	.section	.rodata.str1.1,"aMS",@progbits,1
.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.1:
	.asciz	"research/reports/legacy-performance/2026-10-25-legacy-branchless-return/selection-probe.rs"
	.size	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.1, 91

	.type	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.2,@object
	.section	.data.rel.ro..Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.2,"aw",@progbits
	.p2align	3, 0x0
.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.2:
	.quad	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.1
	.asciz	"Z\000\000\000\000\000\000\0000\000\000\000\016\000\000"
	.size	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.2, 24

	.type	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.3,@object
	.section	.data.rel.ro..Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.3,"aw",@progbits
	.p2align	3, 0x0
.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.3:
	.quad	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.1
	.asciz	"Z\000\000\000\000\000\000\000l\000\000\000\016\000\000"
	.size	.Lanon.a8933c9cc1e3cbc1e5c709aae852dc9e.3, 24

	.ident	"rustc version 1.97.0 (2d8144b78 2026-07-07)"
	.section	".note.GNU-stack","",@progbits
