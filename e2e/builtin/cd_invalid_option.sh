#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd with an invalid option exits 2 with an error message
# EXPECT_EXIT: 2
# EXPECT_STDERR: invalid option
cd -x
