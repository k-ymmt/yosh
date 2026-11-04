#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: An unknown option to export, readonly or trap is a usage error (status 2) that ends a non-interactive shell
# EXPECT_OUTPUT<<END
# yosh: export: -Q: invalid option
# 2
# yosh: readonly: -n: invalid option
# 2
# yosh: trap: -l: invalid option
# 2
# 2
# bad condition kept going
# END
# EXPECT_EXIT: 0
(export -Q x; echo continued) 2>&1; echo $?
(readonly -n x; echo continued) 2>&1; echo $?
(trap -l; echo continued) 2>&1; echo $?
(command trap -l 2>/dev/null; echo $?)
(trap 'echo x' NOSUCHSIG 2>/dev/null; echo bad condition kept going)
