#!/bin/sh
# POSIX_REF: 2.6.7 Quote Removal
# DESCRIPTION: Quotes are removed after all expansions
# EXPECT_OUTPUT<<END
# hello world
# $notavar
# END
x=hello
echo "$x"' world'
echo '$notavar'
