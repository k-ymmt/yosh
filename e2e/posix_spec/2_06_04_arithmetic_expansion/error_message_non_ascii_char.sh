#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: An arithmetic syntax error names a non-ASCII character whole, not its first byte
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
msg=$( (echo $((é+1))) 2>&1 )
case $msg in
    *"'é'"*) echo ok ;;
    *) echo "bad: $msg" ;;
esac
