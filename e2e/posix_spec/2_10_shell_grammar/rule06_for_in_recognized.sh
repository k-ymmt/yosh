#!/bin/sh
# POSIX_REF: 2.10.2 Rule 6 - Third word of for/case recognized as 'in'
# DESCRIPTION: The keyword 'in' is recognized as the third word of for
# EXPECT_OUTPUT<<END
# 1
# 2
# END
# EXPECT_EXIT: 0
for x in 1 2; do
    echo "$x"
done
