#!/bin/sh
# POSIX_REF: 4 Utilities - ulimit
# DESCRIPTION: ulimit supports -H, -S, -a, -c, -d, -n, -s, -t and -v
# EXPECT_OUTPUT<<END
# 0
# 7
# 0
# 64
# 64
# ok
# END
# EXPECT_EXIT: 0
ulimit -a >/dev/null; echo $?
ulimit -a | wc -l | tr -d ' '
ulimit -c 0; ulimit -c
ulimit -n 64
ulimit -Sn
ulimit -Hn
for o in -d -s -t -v -Hc -Sf; do ulimit $o >/dev/null || exit 1; done
echo ok
