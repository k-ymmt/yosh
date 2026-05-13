#!/bin/sh
# POSIX_REF: 2.5.1 Positional Parameters
# DESCRIPTION: ${10} accesses tenth positional; $10 is $1 followed by literal 0
# EXPECT_OUTPUT<<END
# ten
# one0
# END
# EXPECT_EXIT: 0
set -- one two three four five six seven eight nine ten
echo "${10}"
echo "$10"
