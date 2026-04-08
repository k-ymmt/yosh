#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: <<- strips leading tabs
# EXPECT_OUTPUT<<END
# hello
# world
# END
cat <<-EOF
	hello
	world
	EOF
