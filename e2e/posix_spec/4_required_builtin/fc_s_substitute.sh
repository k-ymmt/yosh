#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc -s old=new RE re-executes the most-recent matching command with substitution
# XFAIL: harness limitation (fc -s substitution may rely on interactive history capture)
# EXPECT_EXIT: 0
echo onevar
fc -s one=two echo
