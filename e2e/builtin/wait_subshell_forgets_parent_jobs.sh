#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment / wait
# DESCRIPTION: Subshell cannot wait the parent's finished background job (bash/dash parity)
# EXPECT_OUTPUT<<END
# SUB-127
# MAIN-7
# END
sh -c 'exit 7' & p=$!
sleep 0.2
(wait $p 2>/dev/null; echo SUB-$?)
wait $p; echo MAIN-$?
