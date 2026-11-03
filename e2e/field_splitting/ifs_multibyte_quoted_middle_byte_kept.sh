#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: A multibyte IFS character whose middle byte is quoted is not a delimiter and no byte is lost
# EXPECT_OUTPUT<<END
# <a€b>
# END
# EXPECT_EXIT: 0
# The three bytes of the euro sign come from separate substitutions and
# only the middle one is quoted: an unquoted delimiter needs every byte
# unquoted, so this is one field (bash in a UTF-8 locale).
IFS=€
printf '<%s>\n' $(printf 'a\342')"$(printf '\202')"$(printf '\254b')
