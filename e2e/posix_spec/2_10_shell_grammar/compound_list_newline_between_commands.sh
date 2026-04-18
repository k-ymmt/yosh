#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: compound_list accepts newlines between commands inside if/then/fi
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
if true
then
    echo a
    echo b
fi
