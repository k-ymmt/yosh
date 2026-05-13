#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read with no args is an error
# XFAIL: not yet implemented (TODO: implement read)
# EXPECT_EXIT: 1
# EXPECT_STDERR: read
read 2>&1 1>/dev/null
