#!/bin/sh
# POSIX_REF: 2.9.4.3 Case Conditional Construct
# DESCRIPTION: ;& falls through into the next clause without pattern matching
# EXPECT_OUTPUT<<END
# one
# two
# END
# EXPECT_EXIT: 0
case a in
a) echo one ;&
b) echo two ;;
c) echo three ;;
esac
