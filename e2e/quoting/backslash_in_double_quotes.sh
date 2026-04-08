#!/bin/sh
# POSIX_REF: 2.2.3 Double-Quotes
# DESCRIPTION: In double quotes, backslash only escapes $ backslash double-quote newline
# EXPECT_OUTPUT: $HOME
echo "\$HOME"
