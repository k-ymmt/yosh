#!/bin/sh
# POSIX_REF: 2.14.7 set
# DESCRIPTION: set -e ignores the failure of a pipeline beginning with !
# EXPECT_OUTPUT: after
# EXPECT_EXIT: 0
set -e
! true
echo after
