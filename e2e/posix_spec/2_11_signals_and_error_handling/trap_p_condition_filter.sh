#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: trap -p CONDITION prints only the named condition
# EXPECT_OUTPUT: trap -- 'echo a' SIGUSR1
# EXPECT_EXIT: 0
# XFAIL: yosh ignores the condition operand and prints all traps
trap 'echo a' USR1
trap 'echo b' USR2
trap -p USR1
