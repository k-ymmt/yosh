#!/bin/sh
# POSIX_REF: 2.6.2 Case Conditional Construct / 2.13.1 Patterns
# DESCRIPTION: Quoted glob metacharacters in case patterns match literally
# EXPECT_OUTPUT<<END
# other
# lit
# var_other
# END
x=ab
case $x in "a*") echo lit;; *) echo other;; esac
y='a*'
case $y in "a*") echo lit;; *) echo other;; esac
p='*'
case $x in "$p") echo var_lit;; *) echo var_other;; esac
