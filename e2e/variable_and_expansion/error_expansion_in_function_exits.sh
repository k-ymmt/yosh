#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors (expansion error)
# DESCRIPTION: ${var:?} inside a function exits a non-interactive shell
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: novar: msg
f() {
    echo ${novar:?msg}
    echo in_f
}
f
echo after_f
