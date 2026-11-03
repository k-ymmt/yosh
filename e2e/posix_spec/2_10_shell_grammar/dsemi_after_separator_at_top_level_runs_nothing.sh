#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: ';;' after a separator outside a case is a syntax error for the whole line
# EXPECT_OUTPUT<<END
# 1
# END
# EXPECT_EXIT: 2
# EXPECT_STDERR: ;;
echo 1
echo 2; ;; echo 3
echo 4
