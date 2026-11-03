#!/bin/sh
# POSIX_REF: 2.10.2 Shell Grammar Rules
# DESCRIPTION: A command followed by another command without a separator is a syntax error
# EXPECT_OUTPUT<<END
# 2
# 2
# 2
# 2
# 2
# 2
# 2
# END
# EXPECT_EXIT: 0
for c in 'echo a (echo b)' '{ echo a; } echo b' '(echo a) echo b' 'if true; then echo a; fi echo b' 'f() { :; } (echo y)' '(echo a) fi' 'echo a ;;'; do
    (eval "$c") 2>/dev/null
    echo "$?"
done
