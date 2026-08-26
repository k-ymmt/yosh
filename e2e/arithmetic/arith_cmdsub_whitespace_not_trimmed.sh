#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Command-substitution output in arithmetic keeps interior whitespace (only trailing newlines stripped) — 1<space>2 is a syntax error like bash/dash
# EXPECT_STDERR: syntax error
# EXPECT_EXIT: 1
echo "$((1$(printf ' ')2))"
