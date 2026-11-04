#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: A subshell whose only command is an external utility execs it in place; other shapes keep the subshell
# EXPECT_OUTPUT<<END
# cmdsub ppid ok
# paren ppid ok
# pipe ppid ok
# a b
# st 5
# neg 0
# andor 6
# sig 143
# nf 127
# redir 1
# END
# EXPECT_EXIT: 0
trap 'echo parent exit trap must not run in subshells' EXIT
p=$(/bin/sh -c 'echo $PPID')
[ "$p" = "$$" ] && echo "cmdsub ppid ok"
tmp=$(mktemp)
( /bin/sh -c 'echo $PPID' ) >"$tmp"
read -r p <"$tmp"; rm -f "$tmp"
[ "$p" = "$$" ] && echo "paren ppid ok"
/bin/sh -c 'echo $PPID' | { read -r p; [ "$p" = "$$" ] && echo "pipe ppid ok"; }
x=$(/bin/echo a; /bin/echo b); echo $x
x=$(/bin/sh -c 'exit 5'); echo "st $?"
x=$(! /bin/sh -c 'exit 5'); echo "neg $?"
x=$(/bin/sh -c 'exit 6' || /bin/sh -c 'exit 6'); echo "andor $?"
x=$(/bin/sh -c 'kill -TERM $$'); echo "sig $?"
( nonexistent_cmd_zz_e2e ) 2>/dev/null; echo "nf $?"
( /bin/echo x >/nonexistent_dir_zz/f ) 2>/dev/null; echo "redir $?"
trap - EXIT
