#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar - Pipe Continuation
# DESCRIPTION: Newline after | continues the pipeline implicitly
# EXPECT_OUTPUT: A
# EXPECT_EXIT: 0
echo a |
    tr a A
