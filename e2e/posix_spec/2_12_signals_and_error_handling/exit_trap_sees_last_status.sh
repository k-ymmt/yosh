#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: The EXIT trap runs in the environment of the last executed command; $? is visible
# EXPECT_OUTPUT: st=1
# EXPECT_EXIT: 1
trap 'echo "st=$?"' EXIT
false
