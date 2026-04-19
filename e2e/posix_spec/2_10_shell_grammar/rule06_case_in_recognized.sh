#!/bin/sh
# POSIX_REF: 2.10.2 Rule 6 - Third word of for/case recognized as 'in'
# DESCRIPTION: The keyword 'in' is recognized as the third word of case
# EXPECT_OUTPUT: y
# EXPECT_EXIT: 0
case x in
    x) echo y ;;
esac
