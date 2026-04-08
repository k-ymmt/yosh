#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Nonexistent command returns exit code 127
# EXPECT_EXIT: 127
nonexistent_cmd_xyzzy_12345
