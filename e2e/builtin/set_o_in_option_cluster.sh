#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: -o inside an option cluster takes the next word as the option name
# EXPECT_OUTPUT<<END
# on
# off
# END
# EXPECT_EXIT: 0
set -eo noglob
case $- in *f*) echo on ;; *) echo off ;; esac
set +eo noglob
case $- in *f*) echo on ;; *) echo off ;; esac
