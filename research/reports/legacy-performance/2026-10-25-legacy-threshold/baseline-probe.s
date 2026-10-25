	.file	"baseline_probe.b4af162241522a28-cgu.0"
	.section	.text.selection,"ax",@progbits
	.globl	selection
	.p2align	4
	.type	selection,@function
selection:
	.cfi_startproc
	movzbl	(%rsi), %eax
	testb	%al, %al
	sete	%cl
	cmpb	$32, %al
	setb	%r8b
	andb	%dil, %r8b
	orb	%cl, %r8b
	incb	%al
	movzbl	%al, %eax
	xorl	%ecx, %ecx
	testb	%r8b, %r8b
	cmovnel	%eax, %ecx
	movb	%cl, (%rsi)
	je	.LBB0_1
	jmpq	*%rdx
.LBB0_1:
	retq
.Lfunc_end0:
	.size	selection, .Lfunc_end0-selection
	.cfi_endproc

	.ident	"rustc version 1.97.0 (2d8144b78 2026-07-07)"
	.section	".note.GNU-stack","",@progbits
