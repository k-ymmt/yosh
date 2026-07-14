#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables - IFS
# DESCRIPTION: IFS is set to <space><tab><newline> at startup even when inherited from the environment
# EXPECT_OUTPUT: 2
# EXPECT_EXIT: 0
IFS=x ./target/debug/yosh -c 'v="a b"; set -- $v; echo $#'
