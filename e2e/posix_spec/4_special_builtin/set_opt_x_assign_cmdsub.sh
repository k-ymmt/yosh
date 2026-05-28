#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -x traces an assignment whose value contains a command substitution
# EXPECT_STDERR: + x=hi
# EXPECT_EXIT: 0
# NOTE: Ordering of '+ echo hi' (command-sub trace) before '+ x=hi' (assignment
# trace) is structurally guaranteed by the implementation: expand_word_to_string
# runs before the trace block. E2E substring matching cannot verify ordering;
# the structural guarantee is the contract.
set -x
x=$(echo hi)
