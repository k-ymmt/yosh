#!/bin/sh
# POSIX_REF: 4 Utilities - unalias
# DESCRIPTION: unalias of an undefined name is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: unalias
unalias nosuch
