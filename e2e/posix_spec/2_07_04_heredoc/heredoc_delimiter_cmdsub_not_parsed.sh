#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: A command substitution spelled in a here-document delimiter is never run, so its body need not be a valid program
# EXPECT_OUTPUT<<END
# quoted
# unquoted
# backquoted
# nested
# END
# EXPECT_EXIT: 0
# The delimiter only undergoes quote removal (dash and bash agree on the
# quoted forms; dash rejects an unquoted $( in a delimiter, bash accepts it).
cat <<"$(if)"
quoted
$(if)
cat <<$(if (x) fi)
unquoted
$(if (x) fi)
cat <<"`if`"
backquoted
`if`
cat <<"${x:-$(( $(if) ))}"
nested
${x:-$(( $(if) ))}
