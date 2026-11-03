#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: An unexpanded tilde-prefix (unknown login name) stays literal text: field-split inside ${x:-word} and subject to pathname expansion
# EXPECT_OUTPUT<<END
# <~__yosh_missing_user__>
# <a>
# <~__yosh_missing_userZ1>
# <~__yosh_missing_userZ1>
# END
# EXPECT_EXIT: 0
unset x
printf '<%s>\n' ${x:-~__yosh_missing_user__ a}
d=$(mktemp -d) || exit 1
cd "$d" || exit 1
: > '~__yosh_missing_userZ1'
printf '<%s>\n' ~__yosh_missing_user* ${x:-~__yosh_missing_user*}
cd / && rm -rf "$d"
