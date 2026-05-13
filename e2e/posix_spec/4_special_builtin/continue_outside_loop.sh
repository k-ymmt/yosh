#!/bin/sh
# POSIX_REF: 2.14.5 continue
# DESCRIPTION: continue outside any loop is treated as not-in-loop
# XFAIL: non-POSIX deviation (yosh exits 0 and emits no stderr for continue outside a loop)
# EXPECT_EXIT: 1
# EXPECT_STDERR: continue
continue
