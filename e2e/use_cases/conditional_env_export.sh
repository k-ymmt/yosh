#!/bin/sh
# POSIX_REF: 2.14.9 export
# DESCRIPTION: Use case - export a computed variable so a child shell inherits it
# EXPECT_OUTPUT<<END
# child sees mode=production
# child sees unexported=
# END
MODE=production
export MODE
UNEXPORTED=secret
sh -c 'echo "child sees mode=$MODE"'
sh -c 'echo "child sees unexported=$UNEXPORTED"'
