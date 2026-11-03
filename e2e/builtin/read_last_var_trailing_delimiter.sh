#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: A single trailing delimiter after the last field is not assigned; extra fields keep theirs
# EXPECT_OUTPUT<<END
# [a][b]
# [a][b::]
# [a][b:c:]
# [a]
# [a][]
# END
# EXPECT_EXIT: 0
echo a:b: | { IFS=: read x y; echo "[$x][$y]"; }
echo a:b:: | { IFS=: read x y; echo "[$x][$y]"; }
echo a:b:c: | { IFS=: read x y; echo "[$x][$y]"; }
echo "a :" | { IFS=": " read x; echo "[$x]"; }
echo "a::" | { IFS=": " read x y; echo "[$x][$y]"; }
