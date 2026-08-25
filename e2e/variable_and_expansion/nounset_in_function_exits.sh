#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors (expansion error, set -u)
# DESCRIPTION: set -u expansion error inside a function exits a non-interactive shell
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: parameter not set
set -u
f() {
    echo $undef
    echo in_f
}
f
echo after_f
