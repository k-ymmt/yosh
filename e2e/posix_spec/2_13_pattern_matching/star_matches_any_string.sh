#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: '*' in a case pattern matches any string including empty
# EXPECT_OUTPUT<<END
# caught empty
# caught hello
# END
# EXPECT_EXIT: 0
for arg in "" hello; do
    case "$arg" in
        *) echo "caught ${arg:-empty}" ;;
    esac
done
