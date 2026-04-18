#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd rejects more than one operand
# EXPECT_EXIT: 1
# EXPECT_STDERR: too many arguments
cd /tmp /etc
