#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: The end of an unparsed command substitution in a here-document delimiter ignores parentheses inside backquotes, single quotes, backslash escapes and ${...}
# EXPECT_OUTPUT<<END
# backquoted
# single-quoted
# escaped
# braced
# nested
# END
# EXPECT_EXIT: 0
# `if` alone is not a valid program, so the body is scanned without the
# grammar (dash agrees on all; bash rejects the ${...} form).
cat <<"$(if `(`)"
backquoted
$(if `(`)
cat <<"$(if '(')"
single-quoted
$(if '(')
cat <<"$(if \()"
escaped
$(if \()
cat <<"$(if ${x-(})"
braced
$(if ${x-(})
cat <<"$(if $(()))"
nested
$(if $(()))
