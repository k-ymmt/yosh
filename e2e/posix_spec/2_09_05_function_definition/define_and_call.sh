#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function defined with name() { body; } is callable as a simple command
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
greet() { echo hi; }
greet
