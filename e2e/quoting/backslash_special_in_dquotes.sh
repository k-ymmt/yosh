#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: Inside double quotes only \$ \` \" \\ \newline are special
# EXPECT_OUTPUT<<END
# $HOME
# "quoted"
# back\slash
# END
echo "\$HOME"
echo "\"quoted\""
echo "back\\slash"
