#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: Redirection error on special builtin causes non-interactive shell to exit
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
# Run in subshell so the parent stays alive. The subshell exits non-zero.
(: < /nonexistent/path 2>/dev/null; echo not-reached) ; :
