#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Quoted delimiter suppresses expansion in here-document
# EXPECT_OUTPUT: value is $x
x=hello
cat <<'EOF'
value is $x
EOF
