#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: In an unquoted here-document a trailing backslash joins the next line before the delimiter check; quoted bodies keep it
# EXPECT_OUTPUT<<END
# line E
# a 	b
# x\
# q \
# x
# \
# y
# END
cat <<E
line \
E
E
cat <<-E
	a \
	b
	E
cat <<E
x\\
E
cat <<'E'
q \
E
cat <<E
\
x
E
cat <<'E'
\
y
E
