#!/bin/sh
# POSIX_REF: 2.15 Special Built-In Utilities (eval, dot)
# DESCRIPTION: eval and dot parse and run their input one complete command at a time: aliases apply to later lines and a later syntax error does not undo earlier lines
# EXPECT_OUTPUT<<END
# hi
# dot
# one
# 2
# uno
# 2
# t1
# END
# EXPECT_EXIT: 0
eval 'alias q=echo
q hi'
tmp=${TMPDIR:-/tmp}/yosh_eval_dot_inc_$$
printf 'alias r=echo\nr dot\n' > "$tmp"
. "$tmp"
eval 'echo one
echo two; fi
echo three' 2>/dev/null
echo $?
printf 'echo uno\necho dos; fi\necho tres\n' > "$tmp"
. "$tmp" 2>/dev/null
echo $?
rm -f "$tmp"
trap 'echo t1
echo t2; fi' EXIT
exec 2>/dev/null
