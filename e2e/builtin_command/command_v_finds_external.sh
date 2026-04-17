#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -v reports absolute path for externals
# EXPECT_OUTPUT: /bin/sh
# EXPECT_EXIT: 0
PATH=/bin:/usr/bin command -v sh
