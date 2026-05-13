#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read strips leading and trailing IFS whitespace
# XFAIL: not yet implemented (TODO: implement read)
# EXPECT_OUTPUT: <hello>
# EXPECT_EXIT: 0
echo '   hello   ' | { read line; echo "<$line>"; }
