#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A variable-assignment error causes a non-interactive shell to exit
# EXPECT_OUTPUT<<END
# did-not-continue
# exited-nonzero
# END
# EXPECT_EXIT: 0
out=$(./target/debug/yosh -c 'readonly r=1; r=2; echo unreached' 2>/dev/null)
st=$?
case $out in *unreached*) ;; *) echo did-not-continue ;; esac
[ "$st" -ne 0 ] && echo exited-nonzero
