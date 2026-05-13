#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands
# DESCRIPTION: while loops while condition is true
# EXPECT_OUTPUT<<END
# 0
# 1
# 2
# END
# EXPECT_EXIT: 0
i=0
while [ $i -lt 3 ]; do
    echo $i
    i=$((i+1))
done
