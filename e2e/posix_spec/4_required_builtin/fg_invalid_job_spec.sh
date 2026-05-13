#!/bin/sh
# POSIX_REF: 4 Utilities - fg
# DESCRIPTION: fg with malformed job spec is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: fg
fg %notajob >/dev/null
