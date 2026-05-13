#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts returns nonzero when no options remain
# EXPECT_EXIT: 1
set -- arg
getopts a opt
