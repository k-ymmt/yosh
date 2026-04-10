#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: VAR=val func — prefix assignment scoped to function
# EXPECT_OUTPUT<<END
# in-func
# original
# END
MY_VAR=original
show_var() { echo "$MY_VAR"; }
MY_VAR=in-func show_var
echo "$MY_VAR"
