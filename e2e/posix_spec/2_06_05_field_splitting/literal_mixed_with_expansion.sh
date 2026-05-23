#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Literal text stays intact; only $var expansion is split (one printf line per argv field)
# EXPECT_OUTPUT<<END
# [a::b]
# [x]
# [y]
# [c::d]
# END
# EXPECT_EXIT: 0
IFS=:
v=x:y
printf "[%s]\n" a::b $v c::d
