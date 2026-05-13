#!/bin/sh
# POSIX_REF: 2.14.14 dot
# DESCRIPTION: dot of a nonexistent file is an error
# EXPECT_EXIT: 1
. /no/such/file 2>/dev/null
