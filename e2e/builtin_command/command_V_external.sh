#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -V reports "X is /path" for externals
# EXPECT_OUTPUT: sh is /bin/sh
# EXPECT_EXIT: 0
PATH=/bin:/usr/bin command -V sh
