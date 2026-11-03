#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: IFS whitespace on either side of a non-whitespace IFS delimiter is part of that delimiter
# EXPECT_OUTPUT<<END
# [a][b][]
# [a][b]
# [a][][b]
# END
# EXPECT_EXIT: 0
echo "a :b" | { IFS=" :" read x y z; echo "[$x][$y][$z]"; }
echo "a : b" | { IFS=" :" read x y; echo "[$x][$y]"; }
echo "a: :b" | { IFS=" :" read x y z; echo "[$x][$y][$z]"; }
