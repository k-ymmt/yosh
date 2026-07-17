#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Conditional, length, and strip parameter forms apply in unquoted heredoc bodies
# EXPECT_OUTPUT<<END
# default
# 5
# file
# END
# EXPECT_EXIT: 0
unset x
v=hello
f=file.txt
cat <<EOF
${x:-default}
${#v}
${f%.txt}
EOF
