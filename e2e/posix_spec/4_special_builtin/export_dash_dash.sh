#!/bin/sh
# POSIX_REF: 2.14.9 export (XBD 12.2 Guideline 10)
# DESCRIPTION: export -- treats following operands as names; -- itself is consumed
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
export -- foo=hi || exit 99
sh -c 'echo "$foo"'
