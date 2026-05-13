#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $0 expands to the shell or script name (non-empty)
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
case "$0" in
    '') echo "empty \$0" ;;
    *) echo ok ;;
esac
