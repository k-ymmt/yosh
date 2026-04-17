#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -V on unknown name exits with nonzero status
# EXPECT_EXIT: 1
command -V definitely_not_a_real_cmd_xyz 2>/dev/null
