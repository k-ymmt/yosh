#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: Error diagnostics that embed user data print its original bytes, not the shell's internal escape characters
# EXPECT_OUTPUT<<END
# cd ok
# redirect ok
# END
d=$(printf 'd\377')
msg=$(cd "$d" 2>&1)
case $msg in
"yosh: cd: $d: "*) echo cd ok ;;
*) printf 'cd bad: %s\n' "$msg" | od -c ;;
esac
msg=$({ : >"/nonexistent_dir_zz/$d"; } 2>&1)
case $msg in
"yosh: /nonexistent_dir_zz/$d: "*) echo redirect ok ;;
*) printf 'redirect bad: %s\n' "$msg" | od -c ;;
esac
