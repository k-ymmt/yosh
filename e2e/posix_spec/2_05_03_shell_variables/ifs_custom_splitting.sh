#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: IFS=':' splits on colon only
# EXPECT_OUTPUT<<END
# a|b|c
# END
# EXPECT_EXIT: 0
IFS=:
set -- $(printf 'a:b:c')
IFS=' '
echo "$*" | tr ' ' '|'
