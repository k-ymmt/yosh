#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: A } inside nested $(...) / backticks / ${...} does not close the outer ${...} in a heredoc body
# EXPECT_OUTPUT<<END
# }
# }
# [fallback]
# END
unset x y
cat <<XEOF
${x:-$(printf %s })}
${x:-`printf %s }`}
[${x:-${y:-fallback}}]
XEOF
