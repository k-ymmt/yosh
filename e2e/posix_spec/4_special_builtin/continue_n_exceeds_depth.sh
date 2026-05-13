#!/bin/sh
# POSIX_REF: 2.14.5 continue
# DESCRIPTION: continue with n exceeding nesting acts as continue against outermost loop
# XFAIL: non-POSIX deviation (yosh treats continue with n exceeding depth as break, outputting only 'a')
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
for i in a b; do
    echo $i
    continue 5
done
