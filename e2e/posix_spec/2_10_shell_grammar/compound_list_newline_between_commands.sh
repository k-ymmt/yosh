#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list
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
