#!/bin/sh
# POSIX_REF: 2.9.1.4 Command Search and Execution
# DESCRIPTION: A special built-in is found before functions in command search
# EXPECT_OUTPUT: not-shadowed
# EXPECT_EXIT: 0
out=$(./target/debug/yosh -c 'eval() { echo func; }; eval "echo real"' 2>/dev/null)
[ "$out" != func ] && echo not-shadowed
