#!/bin/sh
# POSIX_REF: 2.9.4.1 if Conditional Construct
# DESCRIPTION: Nested if statements
# EXPECT_OUTPUT: inner
if true; then
  if false; then
    echo wrong
  else
    echo inner
  fi
fi
