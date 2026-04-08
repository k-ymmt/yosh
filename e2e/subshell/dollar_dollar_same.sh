#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $$ in subshell is same as parent shell PID
# EXPECT_OUTPUT: same
parent_pid=$$
child_pid=$(echo $$)
if test "$parent_pid" = "$child_pid"; then
  echo same
else
  echo different
fi
