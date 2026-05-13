#!/bin/sh
# POSIX_REF: 2.14.5 continue
# DESCRIPTION: continue outside any loop is treated as not-in-loop
# EXPECT_EXIT: 1
# EXPECT_STDERR: continue
continue
