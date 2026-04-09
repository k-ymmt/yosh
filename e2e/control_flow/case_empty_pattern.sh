#!/bin/sh
# POSIX_REF: 2.9.4.5 case Conditional Construct
# DESCRIPTION: Case matches empty string pattern
# EXPECT_OUTPUT<<END
# empty
# not-empty
# END
x=
case "$x" in
  '') echo empty ;;
  *) echo fail ;;
esac
x=hello
case "$x" in
  '') echo fail ;;
  *) echo not-empty ;;
esac
