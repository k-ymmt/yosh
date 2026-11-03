#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: An expansion error exits the shell with a non-zero status even when the failing command would report 0
# EXPECT_OUTPUT<<END
# assign rc=1
# for rc=1
# func rc=1
# case rc=1
# END
( set -u; x=$zz; echo after ) 2>/dev/null; echo "assign rc=$?"
( for i in ${zz:?m}; do :; done; echo after ) 2>/dev/null; echo "for rc=$?"
( f() { x=${zz:?m}; }; f; echo after ) 2>/dev/null; echo "func rc=$?"
( case ${zz:?m} in *) : ;; esac; echo after ) 2>/dev/null; echo "case rc=$?"
