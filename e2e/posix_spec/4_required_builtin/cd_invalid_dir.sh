#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd to nonexistent directory is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: cd
cd /no/such/directory
