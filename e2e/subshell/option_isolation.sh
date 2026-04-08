#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Shell option changes in subshell do not affect parent
# EXPECT_OUTPUT: *
(set -f; echo *)
