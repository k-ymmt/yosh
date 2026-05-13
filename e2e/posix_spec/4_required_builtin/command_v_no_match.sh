#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: command -v on nonexistent name exits nonzero with no output
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
command -v /no/such/cmd_$$
