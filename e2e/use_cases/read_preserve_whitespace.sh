#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: Use case - read lines verbatim preserving leading whitespace and backslashes
# EXPECT_OUTPUT<<END
# [  indented line]
# [back\slash]
# [plain]
# END
while IFS= read -r line; do
  printf '[%s]\n' "$line"
done <<EOF
  indented line
back\slash
plain
EOF
