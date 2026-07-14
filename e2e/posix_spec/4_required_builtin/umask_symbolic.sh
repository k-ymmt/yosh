#!/bin/sh
# POSIX_REF: umask utility
# DESCRIPTION: umask accepts a symbolic mode operand
# EXPECT_OUTPUT: 0027
# EXPECT_EXIT: 0
umask u=rwx,g=rx,o=
umask
