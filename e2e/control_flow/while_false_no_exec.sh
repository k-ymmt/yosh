#!/bin/sh
# POSIX_REF: 2.9.4.3 while Loop
# DESCRIPTION: while with initially false condition never executes body
# EXPECT_OUTPUT:
while false; do echo never; done
