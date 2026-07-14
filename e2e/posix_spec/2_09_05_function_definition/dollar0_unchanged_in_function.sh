#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: $0 is unchanged during function invocation
# EXPECT_OUTPUT: unchanged
# EXPECT_EXIT: 0
before=$0
f() { [ "$0" = "$before" ] && echo unchanged; }
f
