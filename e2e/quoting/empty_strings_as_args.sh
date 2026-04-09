#!/bin/sh
# POSIX_REF: 2.2 Quoting
# DESCRIPTION: "" and '' preserve empty arguments in argument lists
# EXPECT_OUTPUT<<END
# 4
# 3
# END
set -- a "" '' b
echo "$#"
set -- "" x ""
echo "$#"
