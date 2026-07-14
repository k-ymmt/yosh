#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Two here-document operators on one line are gathered in order
# EXPECT_OUTPUT<<END
# b-body
# a-body
# END
# EXPECT_EXIT: 0
cat - /dev/fd/3 3<<A <<B
a-body
A
b-body
B
