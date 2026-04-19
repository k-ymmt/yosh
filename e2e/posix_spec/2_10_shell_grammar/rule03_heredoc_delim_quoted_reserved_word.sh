#!/bin/sh
# POSIX_REF: 2.10.2 Rule 3 - Here-document delimiter
# DESCRIPTION: Quoted reserved-word delimiter disables body expansion and still ends at the literal delimiter
# EXPECT_OUTPUT: $X
# EXPECT_EXIT: 0
X=notexpanded
cat <<'if'
$X
if
