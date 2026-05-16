#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -s old=new RE re-executes the most-recent matching command with substitution
# MIGRATED_TO: tests/pty_posix.rs::fc::substitute
# EXPECT_EXIT: 0
echo onevar
fc -s one=two echo
