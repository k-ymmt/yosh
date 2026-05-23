#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: Literal argv tokens are not subject to IFS field splitting (XCU §2.6.5 restricts splitting to expansion results)
# EXPECT_OUTPUT: [a::b]
# EXPECT_EXIT: 0
IFS=:
printf "[%s]\n" a::b
