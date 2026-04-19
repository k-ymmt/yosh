#!/bin/sh
# POSIX_REF: 2.10.2 Rule 4 - Case statement termination
# DESCRIPTION: The last case item may omit ;; before esac
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
case x in
    a) echo a ;;
    x) echo a
esac
