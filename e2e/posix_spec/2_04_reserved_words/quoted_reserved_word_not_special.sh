#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: A quoted reserved word is not recognized and runs as a command
# EXPECT_STDERR: command not found
# EXPECT_EXIT: 127
"done"
