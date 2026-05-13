#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands
# DESCRIPTION: case selects the first matching pattern branch
# EXPECT_OUTPUT: matched
# EXPECT_EXIT: 0
case foo in
    bar) echo no ;;
    foo) echo matched ;;
esac
