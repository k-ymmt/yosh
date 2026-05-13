#!/bin/sh
# POSIX_REF: 2.14.1 break
# DESCRIPTION: break outside any loop is treated as not-in-loop (exit nonzero, message on stderr)
# XFAIL: non-POSIX deviation (yosh exits 0 and emits no stderr for break outside a loop)
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: break
break
