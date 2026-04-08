#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Multi-stage pipeline with three commands
# EXPECT_OUTPUT: HELLO
printf 'hello' | tr a-z A-Z
