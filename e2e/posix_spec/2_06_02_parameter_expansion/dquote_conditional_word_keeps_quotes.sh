#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Inside double quotes and here-documents the word of ${x-word} is scanned with double-quote rules; pattern quoting stays significant
# EXPECT_OUTPUT<<END
# ['a'][\a][$][a b]
# 's'
# 'q'
# 'q'
# 'a' 'a'
# 's' d \a
# bc bc ab
# END
unset x
printf '[%s]' "${x-'a'}" "${x:-\a}" "${x:-\$}" "${x-"a b"}"; echo
echo "${x:-'s'}"
echo "${x:='q'}"; echo "$x"; unset x
y=abc; echo "${y+'a'}" "${y:+'a'}"
cat <<E
${x:-'s'} ${x:-"d"} ${x:-\a}
E
echo "${y#'a'}" "${y#"a"}" "${y%\c}"
