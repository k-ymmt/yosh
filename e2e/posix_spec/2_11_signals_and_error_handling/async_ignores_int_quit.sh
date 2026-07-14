#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: Commands in async lists ignore SIGINT and SIGQUIT when job control is off
# EXPECT_OUTPUT<<END
# int-survived
# quit-survived
# done
# END
# EXPECT_EXIT: 0
( /bin/sh -c 'kill -INT $$; echo int-survived' ) &
wait "$!"
( /bin/sh -c 'kill -QUIT $$; echo quit-survived' ) &
wait "$!"
echo done
