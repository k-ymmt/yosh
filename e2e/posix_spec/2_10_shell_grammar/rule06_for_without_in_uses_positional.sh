#!/bin/sh
# POSIX_REF: 2.10.2 Rule 6 - Third word of for/case recognized as 'in'
# DESCRIPTION: A for with no 'in' word iterates the positional parameters
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
set -- a b
for x do
    echo "$x"
done
