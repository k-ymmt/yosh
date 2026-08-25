#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion / 2.13.1 Patterns
# DESCRIPTION: Quoted glob metacharacters in ${x%pat}/${x#pat} patterns match literally
# EXPECT_OUTPUT<<END
# [abc]
# [abc]
# [abc]
# []
# [ab]
# [abc*]
# END
x=abc
echo "[${x%%"*"}]"
p='*'
echo "[${x%%"$p"}]"
echo "[${x##"?"*}]"
echo "[${x%%$p}]"
y='ab*'
echo "[${y%"*"}]"
z='abc*'
echo "[${z#"?"}]"
