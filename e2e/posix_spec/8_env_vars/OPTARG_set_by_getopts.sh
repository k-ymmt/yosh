#!/bin/sh
# POSIX_REF: 8 Environment Variables - OPTARG
# DESCRIPTION: getopts sets OPTARG to the argument value for options that take an argument
# XFAIL: getopts builtin not yet implemented (TODO: implement getopts)
# EXPECT_OUTPUT: value
# EXPECT_EXIT: 0
set -- -a value
getopts "a:" opt
echo "$OPTARG"
