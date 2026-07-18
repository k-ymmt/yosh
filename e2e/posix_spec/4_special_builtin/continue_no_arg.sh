#!/bin/sh
# POSIX_REF: 2.15 continue
# DESCRIPTION: continue with no operand returns to the top of the innermost loop
# EXPECT_OUTPUT<<END
# 1
# 3
# END
# EXPECT_EXIT: 0
for i in 1 2 3; do
    if [ "$i" = 2 ]; then continue; fi
    echo $i
done
