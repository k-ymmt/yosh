#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Simple two-command pipeline
# EXPECT_OUTPUT: Hello
echo hello | tr h H
