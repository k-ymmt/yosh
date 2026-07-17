#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Assignment prefix before a function definition is an explicit syntax error
# EXPECT_STDERR: syntax error near unexpected token
# EXPECT_EXIT: 2
x=1 f() { :; }
