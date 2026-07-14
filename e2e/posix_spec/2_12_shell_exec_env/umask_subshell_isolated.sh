#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: umask changed in a subshell does not affect the parent
# EXPECT_OUTPUT: 0022
# EXPECT_EXIT: 0
umask 022
( umask 077 )
umask
