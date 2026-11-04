#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: An external command's environment gets the final value of a prefix assignment that a later one's expansion changed
# EXPECT_OUTPUT<<END
# [new]
# [unset]
# 5 5
# [unset]
# [7]
# END
# EXPECT_EXIT: 0
unset x
x= y=${x:=new} sh -c 'echo "[$x]"'
echo "[${x-unset}]"

x=1 y=$((x=5)) sh -c 'echo "$x $y"'
echo "[${x-unset}]"

# The last assignment's value is the one it expanded to.
x=7 sh -c 'echo "[$x]"'
