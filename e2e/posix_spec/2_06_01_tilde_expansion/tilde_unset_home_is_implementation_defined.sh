#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Behavior of tilde with unset HOME is implementation-defined; yosh preserves literal ~
# EXPECT_OUTPUT: ~
# EXPECT_EXIT: 0
unset HOME
echo ~
