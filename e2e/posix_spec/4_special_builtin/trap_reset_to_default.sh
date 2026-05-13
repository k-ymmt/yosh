#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap - SIGNAL resets the trap to default
# EXPECT_OUTPUT: cleared
# EXPECT_EXIT: 0
trap 'echo first' EXIT
trap - EXIT
trap 'echo cleared' EXIT
