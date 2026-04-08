#!/bin/sh
# POSIX_REF: 2.9.4.5 case Conditional Construct
# DESCRIPTION: case matches literal pattern
# EXPECT_OUTPUT: matched
case foo in
  foo) echo matched ;;
  bar) echo wrong ;;
esac
