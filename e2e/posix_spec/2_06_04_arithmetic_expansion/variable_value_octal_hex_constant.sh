#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: A variable's value is read as a C integer constant (octal/hex) by both $((x)) and $(($x))
# EXPECT_OUTPUT<<END
# 8 8
# -8
# 31
# 0 7 42 -7
# END
# EXPECT_EXIT: 0
x=010; echo $((x)) $(($x))
x=-010; echo $((x))
x=0x1f; echo $((x))
a=0 b=07 c=42 d=-7; echo $((a)) $((b)) $((c)) $((d))
