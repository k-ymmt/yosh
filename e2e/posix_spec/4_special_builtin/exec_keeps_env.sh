#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec preserves exported environment
# XFAIL: non-POSIX deviation (yosh exec does not pass exported variables to replaced process)
# EXPECT_OUTPUT: kept
# EXPECT_EXIT: 0
export marker=kept
exec sh -c 'echo "$marker"'
