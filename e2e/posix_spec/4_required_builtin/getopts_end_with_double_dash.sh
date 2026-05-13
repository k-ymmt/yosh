#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts stops at -- and increments OPTIND past it
# EXPECT_EXIT: 1
set -- --
getopts a opt
