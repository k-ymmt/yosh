#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: trap with an unknown signal name is an error
# EXPECT_STDERR: invalid signal name
# EXPECT_EXIT: 1
trap 'echo x' BOGUS
