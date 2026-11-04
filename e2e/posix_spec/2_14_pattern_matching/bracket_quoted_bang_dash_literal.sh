#!/bin/sh
# POSIX_REF: 2.14.1 Patterns Matching a Single Character
# DESCRIPTION: A quoted ! or - inside a bracket expression is a literal member, not negation or a range
# EXPECT_OUTPUT<<END
# m1 nm2 m3 nm4 m5 nm6
# abc bc ab
# END
# EXPECT_EXIT: 0
r=
case '!' in [\!x]) r="$r m1";; *) r="$r nm1";; esac
case a in [\!x]) r="$r m2";; *) r="$r nm2";; esac
case - in [a"-"c]) r="$r m3";; *) r="$r nm3";; esac
case b in [a"-"c]) r="$r m4";; *) r="$r nm4";; esac
case b in ["!"b]) r="$r m5";; *) r="$r nm5";; esac
case a in ["!"b]) r="$r m6";; *) r="$r nm6";; esac
echo $r
x=abc
echo ${x#["!"b]} ${x#[a"-"z]} ${x%[\!c]}
