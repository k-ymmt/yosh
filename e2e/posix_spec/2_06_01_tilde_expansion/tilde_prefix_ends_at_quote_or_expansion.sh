#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: A tilde-prefix containing a quoted char or an expansion is literal; inside ${...} it ends at the closing brace
# EXPECT_OUTPUT<<END
# /hh /hh /z
# ~/q ~/q /hh/q
# ~x
# ~root
# ~root/y
# ~root
# a:~r
# /hh:a
# ~ a
# END
HOME=/hh
unset x
y=/hh/z
echo ${x:-~} ${x:=~} ${y#~}
unset x
echo ~"/q" ~\/q ~/"q"
U=x; echo ~$U
echo ~'root'
echo ~root$(echo /y)
x=~'root'; echo $x
y=r; x=a:~$y; echo $x
x=~:'a'; echo $x
unset x; echo ${x:-~ a}
