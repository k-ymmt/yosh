#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of function (compound_command)
# DESCRIPTION: The function body must be a compound_command; a simple command is not allowed
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
f() echo ok
