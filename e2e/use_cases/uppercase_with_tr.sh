#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Use case - convert a string to upper case by piping through tr
# EXPECT_OUTPUT: HELLO WORLD
printf '%s\n' "hello world" | tr '[:lower:]' '[:upper:]'
