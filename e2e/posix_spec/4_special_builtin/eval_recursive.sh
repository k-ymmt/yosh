#!/bin/sh
# POSIX_REF: 2.15 eval
# DESCRIPTION: eval can recursively invoke eval
# EXPECT_OUTPUT: deep
# EXPECT_EXIT: 0
eval 'eval echo deep'
