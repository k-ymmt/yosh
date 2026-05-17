#!/bin/sh
# POSIX_REF: 4 Utilities - ulimit
# DESCRIPTION: ulimit with unknown option is an error
# XFAIL: deferred (TODO: implement ulimit; out of scope for v0.x — tracked in TODO.md)
# EXPECT_EXIT: 1
# EXPECT_STDERR: ulimit
ulimit -Z 2>&1 1>/dev/null
