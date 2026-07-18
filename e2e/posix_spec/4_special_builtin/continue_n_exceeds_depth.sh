#!/bin/sh
# POSIX_REF: 2.15 continue
# DESCRIPTION: continue with n exceeding nesting acts as continue against outermost loop
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
for i in a b; do
    echo $i
    continue 5
done
