#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: 'command not found' does not exit the shell
# EXPECT_OUTPUT: survived
# EXPECT_EXIT: 0
no_such_command_zzz 2>/dev/null
echo survived
