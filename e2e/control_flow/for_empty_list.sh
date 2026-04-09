#!/bin/sh
# POSIX_REF: 2.9.4.2 for Loop
# DESCRIPTION: for loop with empty list does not execute body
# EXPECT_OUTPUT: done
for i in; do
  echo "should not print"
done
echo done
