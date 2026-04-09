#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Command not found exits with 127
# EXPECT_EXIT: 127
nonexistent_command_xyz_12345
