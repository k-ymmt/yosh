#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: Aliases are substituted in every command-name position: after reserved words, case patterns, operators, and assignments
# EXPECT_OUTPUT<<END
# x1
# x2
# x3
# x4
# x5
# a6
# b6
# ok7
# x8
# x9
# x10
# x11
# x12
# END
# EXPECT_EXIT: 0
alias e=echo
if true; then e x1; fi
{ e x2; }
for i in 1; do e x3; done
case x in x) e x4;; esac
case x in (y) e no;; x) e x5;; esac
true && e a6 && e b6
x=1 e ok7
! e x8
while e x9; false; do :; done
e x10 | cat; e x11 | cat
(e x12)
