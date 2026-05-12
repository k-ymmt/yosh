#!/bin/sh
# POSIX_REF: 2.14.8 export
# DESCRIPTION: export makes variable available to child processes
# EXPECT_EXIT: 0
export MY_EXPORT_TEST=hello
result=$(/usr/bin/env | grep MY_EXPORT_TEST)
test "$result" = "MY_EXPORT_TEST=hello"
