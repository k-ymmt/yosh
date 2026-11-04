#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: A quoted empty string after an IFS delimiter still forms an (empty) field
# EXPECT_OUTPUT<<END
# 2:[a][]
# 2:[a][]
# 1:[]
# 2:[a][]
# 3:[a][][]
# 2:[a][]
# END
# EXPECT_EXIT: 0
x='a '
set -- $x""; echo "$#:[$1][$2]"
set -- $x''; echo "$#:[$1][$2]"
x=' '
set -- $x""; echo "$#:[$1]"
x='a '
set -- ${x}${y-""}; echo "$#:[$1][$2]"
IFS=:
x='a::'
set -- $x""; echo "$#:[$1][$2][$3]"
set -- ""
x='a:'
set -- $x"$@"; echo "$#:[$1][$2]"
