#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Assignment cmd sub propagates $? even when status equals prior cmd
# EXPECT_OUTPUT: after=1
# EXPECT_EXIT: 0
false
x=$(false)
echo "after=$?"
