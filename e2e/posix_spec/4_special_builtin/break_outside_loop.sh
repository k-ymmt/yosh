#!/bin/sh
# POSIX_REF: 2.14.1 break
# DESCRIPTION: break outside any loop is treated as not-in-loop (exit nonzero, message on stderr)
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: break
break
