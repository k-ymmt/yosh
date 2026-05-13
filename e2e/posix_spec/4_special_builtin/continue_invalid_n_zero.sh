#!/bin/sh
# POSIX_REF: 2.14.5 continue
# DESCRIPTION: continue 0 is invalid
# EXPECT_EXIT: 2
# EXPECT_STDERR: continue
for i in 1; do
    continue 0
done
