#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: A closing reserved word after ';' rejects the whole line before any of it runs; earlier lines still ran
# EXPECT_OUTPUT<<END
# line1
# END
# EXPECT_EXIT: 2
# EXPECT_STDERR: fi
echo line1
echo hi; echo b; fi
echo never
