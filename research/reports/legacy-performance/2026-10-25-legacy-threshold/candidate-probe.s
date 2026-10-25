	.file	"candidate_probe.717bfa530c541be3-cgu.0"
	.section	.text.selection,"ax",@progbits
	.globl	selection
	.p2align	4
	.type	selection,@function
selection:
	.cfi_startproc
	testl	%edi, %edi
	movl	$32, %eax
	movl	$1, %ecx
	cmovnel	%eax, %ecx
	movzbl	(%rsi), %eax
	cmpb	%cl, %al
	jae	.LBB0_1
	incb	%al
	movb	%al, (%rsi)
	jmpq	*%rdx
.LBB0_1:
	movb	$0, (%rsi)
	retq
.Lfunc_end0:
	.size	selection, .Lfunc_end0-selection
	.cfi_endproc

	.ident	"rustc version 1.97.0 (2d8144b78 2026-07-07)"
	.section	".note.GNU-stack","",@progbits
