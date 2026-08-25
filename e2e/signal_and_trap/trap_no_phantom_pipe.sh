#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: trap listing shows no phantom SIGPIPE ignore entry
# EXPECT_OUTPUT: end
# EXPECT_EXIT: 0
trap
echo end
