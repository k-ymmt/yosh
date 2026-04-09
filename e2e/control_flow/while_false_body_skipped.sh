#!/bin/sh
# POSIX_REF: 2.9.4.3 while Loop
# DESCRIPTION: while false never executes body
# EXPECT_OUTPUT: done
while false; do
  echo "should not print"
done
echo done
