#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Defining a function with the name of a special built-in is an error
# EXPECT_OUTPUT: def-rejected
# EXPECT_EXIT: 0
# XFAIL: yosh accepts function definitions named after special built-ins
./target/debug/yosh -c 'eval() { echo x; }' 2>/dev/null
[ $? -ne 0 ] && echo def-rejected
