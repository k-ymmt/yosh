#!/bin/sh
# POSIX_REF: 2.14.1 Patterns Matching a Single Character
# DESCRIPTION: Bracket expressions accept [.c.] collating symbols (also as range endpoints) and [=c=] equivalence classes
# EXPECT_OUTPUT<<END
# sym
# equiv
# range
# range2
# nomatch
# dot
# glob: ab
# END
# EXPECT_EXIT: 0
case a in [[.a.]]) echo sym ;; *) echo no ;; esac
case a in [[=a=]]) echo equiv ;; *) echo no ;; esac
case b in [a-[.c.]]) echo range ;; *) echo no ;; esac
case b in [[.a.]-c]) echo range2 ;; *) echo no ;; esac
case d in [a-[.c.]]) echo range ;; *) echo nomatch ;; esac
case . in [[...]]) echo dot ;; *) echo no ;; esac
mkdir -p "$TEST_TMPDIR/cs"
cd "$TEST_TMPDIR/cs" || exit 1
: > ab
: > cb
echo glob: [[.a.]]b
