#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: Redirection error on regular builtin does not exit non-interactive shell
# EXPECT_OUTPUT: continued
# EXPECT_EXIT: 0
true < /nonexistent/path 2>/dev/null
echo continued
