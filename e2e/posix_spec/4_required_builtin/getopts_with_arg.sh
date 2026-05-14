#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts a: opt parses -a value into $OPTARG
# EXPECT_OUTPUT: a=value
# EXPECT_EXIT: 0
set -- -a value
getopts "a:" opt
echo "$opt=$OPTARG"
