#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Use case - print a multi-line templated message with an expanding heredoc
# EXPECT_OUTPUT<<END
# Hello, alice!
# Your build #42 finished.
# Status: success
# END
user=alice
build=42
status=success
cat <<EOF
Hello, $user!
Your build #$build finished.
Status: $status
EOF
