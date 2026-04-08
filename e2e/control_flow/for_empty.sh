#!/bin/sh
# POSIX_REF: 2.9.4.2 for Loop
# DESCRIPTION: for with empty word list does not execute body
# EXPECT_OUTPUT:
for i in; do echo "$i"; done
