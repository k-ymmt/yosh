#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: A trailing pipe with no following command is a syntax error
# EXPECT_STDERR: expected a command after
# EXPECT_EXIT: 2
echo hi |
