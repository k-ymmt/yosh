#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read splits on one-byte and multi-byte IFS characters alike; escaped delimiters stay literal
# EXPECT_OUTPUT<<END
# [a][b][c d]
# [a:b][c]
# [x][y é z]
# [é]
# END
# EXPECT_EXIT: 0
printf 'aéb:c d\n' | { IFS='é:' read x y z; echo "[$x][$y][$z]"; }
printf 'a\\:béc\n' | { IFS='é:' read x y; echo "[$x][$y]"; }
printf ' x y é z \n' | { IFS=' é' read x y; echo "[$x][$y]"; }
printf '\\é\n' | { IFS='é' read x; echo "[$x]"; }
