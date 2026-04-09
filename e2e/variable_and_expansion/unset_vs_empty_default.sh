#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var-default} vs ${var:-default} — unset vs empty distinction
# EXPECT_OUTPUT<<END
# default
# 
# default
# default
# END
unset x
echo "${x-default}"
x=
echo "${x-default}"
unset y
echo "${y:-default}"
y=
echo "${y:-default}"
