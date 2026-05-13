#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: <<- strips leading tab characters from each input line
# EXPECT_OUTPUT: hey
# EXPECT_EXIT: 0
cat <<-EOF
	hey
	EOF
