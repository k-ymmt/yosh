#!/bin/sh
# POSIX_REF: wait utility, EXIT STATUS
# DESCRIPTION: no-operand wait exits 0 even when children failed or $? was nonzero
# EXPECT_OUTPUT<<END
# rc=0
# rc2=0
# END
# EXPECT_EXIT: 0

# POSIX: "The wait utility was invoked with no operands and all process
# IDs known by the invoking shell have terminated" => exit 0. The
# children's own statuses must not leak through (bash/dash agree).
/bin/sh -c 'exit 7' &
wait
echo rc=$?
false
wait
echo rc2=$?
