#!/bin/sh
# POSIX_REF: 2.15 continue
# DESCRIPTION: continue 2 returns to the top of the second enclosing loop
# EXPECT_OUTPUT<<END
# 1-1
# 2-1
# END
# EXPECT_EXIT: 0
for i in 1 2; do
    for j in 1 2; do
        echo $i-$j
        continue 2
    done
done
