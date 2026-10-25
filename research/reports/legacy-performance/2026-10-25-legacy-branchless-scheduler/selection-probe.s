	.file	"selection_probe.6863bc1677bde6c4-cgu.0"
	.section	.text.baseline,"ax",@progbits
	.globl	baseline
	.p2align	4
	.type	baseline,@function
baseline:
	.cfi_startproc
	testl	%edi, %edi
	je	.LBB0_1
	movzbl	(%rsi), %eax
	cmpb	$32, %al
	jb	.LBB0_3
	cmpb	$0, (%rdx)
	jne	.LBB0_6
	jmp	.LBB0_3
.LBB0_1:
	cmpb	$0, (%rdx)
	je	.LBB0_2
.LBB0_6:
	movb	$0, (%rsi)
	movb	$0, (%rdx)
	retq
.LBB0_2:
	movzbl	(%rsi), %eax
.LBB0_3:
	incb	%al
	movzbl	%al, %eax
	movl	$255, %edi
	cmovnel	%eax, %edi
	movb	%dil, (%rsi)
	movb	$1, (%rdx)
	jmpq	*%rcx
.Lfunc_end0:
	.size	baseline, .Lfunc_end0-baseline
	.cfi_endproc

	.section	.text.candidate,"ax",@progbits
	.globl	candidate
	.p2align	4
	.type	candidate,@function
candidate:
	.cfi_startproc
	movzbl	(%rsi), %eax
	cmpb	$32, %al
	setb	%r8b
	andb	%dil, %r8b
	movzbl	(%rdx), %edi
	xorb	$1, %dil
	orb	%r8b, %dil
	incb	%al
	movzbl	%al, %eax
	movl	$255, %r8d
	cmovnel	%eax, %r8d
	xorl	%eax, %eax
	movb	%dil, (%rdx)
	testb	%dil, %dil
	cmovnel	%r8d, %eax
	movb	%al, (%rsi)
	je	.LBB1_1
	jmpq	*%rcx
.LBB1_1:
	retq
.Lfunc_end1:
	.size	candidate, .Lfunc_end1-candidate
	.cfi_endproc

	.ident	"rustc version 1.97.0 (2d8144b78 2026-07-07)"
	.section	".note.GNU-stack","",@progbits
