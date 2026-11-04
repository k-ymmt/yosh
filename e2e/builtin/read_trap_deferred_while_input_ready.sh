#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: A trapped signal that arrives while read has input ready does not end the read, and its action runs once the read has completed
# EXPECT_OUTPUT<<END
# TRAP:<abcdef> status=0
# status=0 x=<abcdef>
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
mkfifo in || exit 1
exec 3<>in
trap 'echo "TRAP:<$x> status=$?"' USR1
x=old
printf 'abc' >&3
# The rest of the line comes later, from a process that is not the
# shell's child: dash ends a read that a SIGCHLD interrupts while any
# signal is pending, which would blur what this test checks.
( (sleep 1; printf 'def\n' >&3) >/dev/null 2>&1 & )
# The command substitution in read's own redirection sends USR1 before
# read starts but after the last point where the shell could run the
# action, so read finds the signal pending and input ready: the signal
# must neither end the read (still waiting for the rest of the line
# once abc is consumed) nor run its action before the line is complete.
read x <&3 2>"$(kill -USR1 $$; echo /dev/null)"
echo "status=$? x=<$x>"
