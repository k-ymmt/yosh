#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: The ! exemption propagates out of an in-process brace group (matches bash/dash)
# EXPECT_OUTPUT: after
# EXPECT_EXIT: 0
set -e
{ ! true; }
echo after
