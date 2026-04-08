#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Functions defined in subshell do not exist in parent
# EXPECT_EXIT: 127
(myfn() { echo hello; })
myfn
