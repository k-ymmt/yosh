#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: Redirection error on a non-special-builtin command fails that command but does not exit the shell
# EXPECT_OUTPUT: after
# EXPECT_EXIT: 0
cat </nonexistent/path/hopefully 2>/dev/null
echo after
