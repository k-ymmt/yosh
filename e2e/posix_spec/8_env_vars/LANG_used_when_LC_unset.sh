#!/bin/sh
# POSIX_REF: 8 Environment Variables - LANG
# DESCRIPTION: LANG is used when LC_ALL and LC_<category> are unset
# EXPECT_EXIT: 0
unset LC_ALL
unset LC_COLLATE
LANG=C
case M in [A-Z]) exit 0 ;; *) exit 1 ;; esac
