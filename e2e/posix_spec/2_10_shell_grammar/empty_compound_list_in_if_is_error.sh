#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty compound_list inside 'if ... then fi' is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
# XFAIL: parser accepts empty compound_list; should be a syntax error per §2.10 BNF (term : term separator and_or | and_or)
if true; then
fi
