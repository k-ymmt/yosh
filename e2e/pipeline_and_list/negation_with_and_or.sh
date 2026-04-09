#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: ! negation with && and || — precedence
# EXPECT_OUTPUT<<END
# yes
# yes
# END
! false && echo yes
! true || echo yes
