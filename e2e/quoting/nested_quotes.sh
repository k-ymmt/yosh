#!/bin/sh
# POSIX_REF: 2.2 Quoting
# DESCRIPTION: Nesting double quotes inside single quotes and vice versa
# EXPECT_OUTPUT<<END
# he said "hello"
# it's fine
# END
echo 'he said "hello"'
echo "it's fine"
