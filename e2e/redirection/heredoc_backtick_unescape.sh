#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document / 2.6.3 Command Substitution
# DESCRIPTION: Backslash before $ inside heredoc backticks is removed (escape, not literal)
# EXPECT_OUTPUT: [a]
cat <<EOF2
[`echo "a\$b"`]
EOF2
