#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: Signals ignored on shell entry remain ignored even after 'trap ... SIGNAL'
# EXPECT_OUTPUT: still_alive
# EXPECT_EXIT: 0
sh -c 'trap "" INT; exec sh -c "trap \"echo trapped\" INT; kill -INT \$\$; echo still_alive"'
