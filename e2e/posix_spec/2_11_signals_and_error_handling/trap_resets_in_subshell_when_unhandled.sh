#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: Subshell starts with traps reset to default for signals not caught in parent
# XFAIL: harness limitation (POSIX trap-in-subshell semantics differ across shells; spec interpretation in flux)
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
trap 'echo parent' USR1
out=$( (trap) )
case "$out" in
    *USR1*) echo unexpected ;;
    *) echo ok ;;
esac
