#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: <<- strips the leading tabs of a logical line, including a continuation line nothing precedes, but not tabs after earlier text
# EXPECT_OUTPUT<<END
# a	b
# x
# done
# END
# EXPECT_EXIT: 0
cat <<-EOF
a\
	b
\
		x
\
	EOF
printf "done\n"
