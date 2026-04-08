#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Pipeline with echo piped to external command
# EXPECT_OUTPUT: HELLO WORLD
echo hello world | tr a-z A-Z
