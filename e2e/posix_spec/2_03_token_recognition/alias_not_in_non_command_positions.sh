#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: Case subjects, case patterns, redirect targets, and reserved words are not alias-substituted
# EXPECT_OUTPUT<<END
# pat1
# pat2
# hi
# kw
# END
# EXPECT_EXIT: 0
alias p='echo pp'
case p in p) echo pat1;; esac
case p in
p) echo pat2
;;
p) echo no
esac
alias out=zz
cd "$TEST_TMPDIR" || exit 1
echo hi >out
cat out
alias if=echo
if true; then echo kw; fi
