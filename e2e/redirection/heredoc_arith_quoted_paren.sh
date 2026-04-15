#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Quoted ')' in $(cmd) must not break heredoc $((...)) boundary detection
# EXPECT_OUTPUT<<END
# 4
# 4
# END
cat <<EOF
$(( $(echo '3)' | cut -c1) + 1 ))
$(( $(echo "3)" | cut -c1) + 1 ))
EOF
