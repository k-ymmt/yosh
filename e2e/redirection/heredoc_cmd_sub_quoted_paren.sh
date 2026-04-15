#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Quoted ')' in $(cmd) must not break heredoc command substitution scanning
# EXPECT_OUTPUT<<END
# 3)
# 3)
# END
cat <<EOF
$(echo '3)')
$(echo "3)")
EOF
