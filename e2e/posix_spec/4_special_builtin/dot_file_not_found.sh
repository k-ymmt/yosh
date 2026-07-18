#!/bin/sh
# POSIX_REF: 2.15 dot
# DESCRIPTION: dot of a nonexistent file is an error
# EXPECT_EXIT: 1
. /no/such/file 2>/dev/null
