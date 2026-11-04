#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: An unknown option after trap -p is a usage error (status 2) that ends a non-interactive shell
# EXPECT_OUTPUT<<END
# yosh: trap: -Z: invalid option
# 2
# 2
# trap -- 'echo hi' USR1
# 0
# END
# EXPECT_EXIT: 0
(trap -p -Z; echo continued) 2>&1; echo $?
(trap -p -Z USR1; echo continued) 2>/dev/null; echo $?
trap 'echo hi' USR1
trap -p -- USR1; echo $?
