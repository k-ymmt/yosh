#!/bin/sh
# POSIX_REF: 4 Utilities - ulimit
# DESCRIPTION: ulimit with unknown option is an error
# XFAIL: not yet implemented (TODO: implement ulimit)
# EXPECT_EXIT: 1
# EXPECT_STDERR: ulimit
ulimit -Z 2>&1 1>/dev/null
