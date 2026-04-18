#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: cd /tmp/../etc resolves to /etc lexically
# EXPECT_OUTPUT: /etc
# EXPECT_EXIT: 0
cd /tmp/../etc
echo "$PWD"
