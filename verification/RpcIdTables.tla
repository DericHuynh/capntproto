---------------------------- MODULE RpcIdTables ----------------------------
EXTENDS Naturals
CONSTANT Fault
\* Two low slots in each of two distinct domains, one adopted question and one
\* high question. High allocation is bounded to one issue; wrap is tested in
\* native Rust separately. Repeated absent removals and rejected insertions are
\* actions, so future allocations must remain correct after those failures.
VARIABLES q, e, h, issued, adopted, event, result, collision, beforeOther, beforeLow
vars == <<q,e,h,issued,adopted,event,result,collision,beforeOther,beforeLow>>
Init == /\ q=0 /\ e=0 /\ h=0 /\ issued=0 /\ adopted=0
        /\ event=0 /\ result=0 /\ collision=0 /\ beforeOther=0 /\ beforeLow=0
Has(mask, slot) == (mask \div 2^slot) % 2 = 1
Clear(mask, slot) == IF Has(mask,slot) THEN mask-2^slot ELSE mask
Push(domain) ==
    /\ (IF domain=1 THEN q ELSE e) # 3
    /\ LET mask == IF domain=1 THEN q ELSE e
           slot == IF Fault="reuse-live" \/ ~Has(mask,0) THEN 0 ELSE 1
           value == IF Has(mask,slot) THEN mask ELSE mask+2^slot
       IN /\ q'=(IF domain=1 THEN value ELSE q)
          /\ e'=(IF domain=2 THEN value ELSE IF Fault="domain" THEN value ELSE e)
          /\ result'=slot+1
          /\ collision'=(IF Has(mask,slot) THEN 1 ELSE collision)
    /\ beforeOther'=(IF domain=1 THEN e ELSE q) /\ beforeLow'=q
    /\ event'=domain
    /\ UNCHANGED <<h,issued,adopted>>
Remove(domain, slot) ==
    /\ result'=(IF Has(IF domain=1 THEN q ELSE e,slot) THEN 1 ELSE 0)
    /\ q'=(IF domain=1 THEN Clear(q,slot) ELSE q)
    /\ e'=(IF domain=2 THEN Clear(e,slot) ELSE e)
    /\ event'=3+2*(domain-1)+slot
    /\ beforeOther'=(IF domain=1 THEN e ELSE q) /\ beforeLow'=q
    /\ UNCHANGED <<h,issued,adopted,collision>>
Sparse(op) ==
    /\ op\in 7..12
    /\ op=7 => issued=0
    /\ h'=(IF op=7 THEN 1 ELSE IF op=8 THEN 0 ELSE h)
    /\ issued'=(IF op=7 THEN 1 ELSE issued)
    /\ adopted'=(IF op=9 THEN IF adopted=0 THEN 1 ELSE
                        IF Fault="replace-adopted" THEN 2 ELSE adopted
                  ELSE IF op=10 THEN 0 ELSE adopted)
    /\ q'=(IF Fault="sparse-low" /\ op=10 THEN Clear(q,0) ELSE q)
    /\ result'=(CASE op=7 -> 4 [] op=8 -> h
                    [] op=9 -> IF adopted=0 THEN 1 ELSE 0
                    [] op=10 -> IF adopted#0 THEN 1 ELSE 0
                    [] OTHER -> 0)
    /\ event'=op /\ beforeOther'=e /\ beforeLow'=q
    /\ UNCHANGED <<e,collision>>
Next == (\E domain\in 1..2: Push(domain))
        \/ (\E domain\in 1..2, slot\in 0..1: Remove(domain,slot))
        \/ (\E op\in 7..12: Sparse(op))
Spec == Init /\ [][Next]_vars
TypeOK == /\ q\in 0..3 /\ e\in 0..3 /\ h\in 0..1 /\ issued\in 0..1
          /\ adopted\in 0..2 /\ event\in 0..12 /\ result\in 0..4
          /\ collision\in 0..1 /\ beforeOther\in 0..3 /\ beforeLow\in 0..3
UniqueAllocation == collision=0
DomainIsolation == event#0 => (IF event\in {2,5,6} THEN q ELSE e)=beforeOther
SparseIsolation == event\in 7..12 => q=beforeLow
AdoptedOwner == adopted\in 0..1
=============================================================================
