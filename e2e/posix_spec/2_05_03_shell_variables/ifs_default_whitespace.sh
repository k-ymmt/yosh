#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: Default IFS splits on space, tab, and newline
# EXPECT_OUTPUT<<END
# 3
# END
# EXPECT_EXIT: 0
set -- $(printf 'a\tb c')
echo $#
