#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: case item bodies can be empty per POSIX BNF
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
case x in
    pat) ;;
esac
echo ok
