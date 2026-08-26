#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: set -e applies inside a signal trap action (bash/dash/zsh agree)
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
set -e
trap 'false' USR1
kill -USR1 $$
echo ok
