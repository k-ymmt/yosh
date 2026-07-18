#!/bin/sh
# POSIX_REF: 2.15 return
# DESCRIPTION: return at script top level (no enclosing function or dot script) is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: return
return
