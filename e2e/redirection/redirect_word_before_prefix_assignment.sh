#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Redirection words are expanded before prefix assignments take effect
# EXPECT_OUTPUT<<END
# foo
# foo2
# foo3
# END
mkdir "$TEST_TMPDIR/d" && cd "$TEST_TMPDIR/d" || exit 1
x=foo
x=bar : > $x
ls
rm -f foo
f() { :; }
x=foo2
x=bar f > $x
ls
rm -f foo2
x=foo3
x=bar cd . > $x
ls
