#!/bin/sh
# POSIX_REF: 2.10.2 Shell Grammar Rules
# DESCRIPTION: A reserved word cannot be a function name (rule 8): esac() is a syntax error
# EXPECT_OUTPUT:
# EXPECT_EXIT: 2
# EXPECT_STDERR: esac
esac() { echo defined; }
echo not-reached
