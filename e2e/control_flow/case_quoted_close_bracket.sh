#!/bin/sh
# POSIX_REF: 2.6.2 Case Conditional Construct / 2.13.1 Patterns
# DESCRIPTION: A quoted ] cannot close a bracket expression in a case pattern - the [ stays literal
# EXPECT_OUTPUT<<END
# ok
# lit
# member
# glob
# END
case src in sr[c"]") echo bad;; *) echo ok;; esac
case 'sr[c]' in sr[c"]") echo lit;; *) echo bad;; esac
case ] in [a"]"b]) echo member;; *) echo bad;; esac
case src in sr[abc]) echo glob;; *) echo bad;; esac
