#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: trap can reference signals by their POSIX name (INT)
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
trap 'echo caught' INT
echo ok
