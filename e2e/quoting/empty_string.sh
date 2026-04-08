#!/bin/sh
# POSIX_REF: 2.2 Quoting
# DESCRIPTION: Empty quotes produce an empty argument (not removed)
# XFAIL: kish drops empty string arguments from set --
# EXPECT_OUTPUT: 3
set -- a "" c
echo "$#"
