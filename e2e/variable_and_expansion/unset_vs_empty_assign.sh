#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var=default} vs ${var:=default} — colon presence with assign
# EXPECT_OUTPUT<<END
# default
# 
# default
# default
# END
unset x
echo "${x=default}"
unset y
y=
echo "${y=default}"
unset a
echo "${a:=default}"
b=
echo "${b:=default}"
