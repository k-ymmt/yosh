#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: <<- strips only leading tabs, not spaces
# EXPECT_OUTPUT<<END
#   space-indented
# tab-then-content
# END
cat <<-EOF
	  space-indented
	tab-then-content
	EOF
