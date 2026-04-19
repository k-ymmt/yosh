#!/bin/sh
# POSIX_REF: 2.10.2 Rule 4 - Case statement termination
# DESCRIPTION: A case item must begin with at least one pattern
# EXPECT_EXIT: 2
# EXPECT_STDERR: yosh:
case x in
    ) echo nothing ;;
esac
