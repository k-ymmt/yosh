#!/bin/sh
# POSIX_REF: 2.15 exec
# DESCRIPTION: exec of a nonexistent command exits 127
# EXPECT_EXIT: 127
exec /no/such/command 2>/dev/null
