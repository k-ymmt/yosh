#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: OLDPWD stores the logical previous PWD
# EXPECT_OUTPUT: /tmp
# EXPECT_EXIT: 0
cd /tmp
cd /etc
echo "$OLDPWD"
