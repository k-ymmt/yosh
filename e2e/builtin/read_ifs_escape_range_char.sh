#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: read treats a UTF-8 IFS character in byteenc's reserved range (U+10FE80) as one delimiter, and IFS raw bytes that form a UTF-8 character as that character (bash agrees)
# EXPECT_OUTPUT<<END
# a|b
# a|b
# END
IFS=$(printf '\364\217\272\200')
printf 'a\364\217\272\200b\n' | { read -r a b; printf '%s|%s\n' "$a" "$b"; }
IFS=$(printf '\303')$(printf '\251')
printf 'a\303\251b\n' | { read -r a b; printf '%s|%s\n' "$a" "$b"; }
