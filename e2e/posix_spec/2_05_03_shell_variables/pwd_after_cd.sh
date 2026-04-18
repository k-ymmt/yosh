#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: PWD reflects the current working directory after cd
# EXPECT_OUTPUT: /tmp
# EXPECT_EXIT: 0
cd /tmp
echo "$PWD"
