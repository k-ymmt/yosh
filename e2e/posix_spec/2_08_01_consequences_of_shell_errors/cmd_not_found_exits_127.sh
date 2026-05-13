#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Unknown command yields exit status 127
# EXPECT_OUTPUT: 127
# EXPECT_EXIT: 0
nonexistent_cmd_xyz 2>/dev/null
echo $?
