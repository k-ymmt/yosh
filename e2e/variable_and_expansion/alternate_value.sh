#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Alternate value with :+ when set vs unset
# EXPECT_OUTPUT<<END
# alt
# -
# END
x=set
echo "${x:+alt}"
unset y
echo "${y:+alt}-"
