#!/bin/sh
# POSIX_REF: 2.7.7 Open File Descriptors for Reading and Writing
# DESCRIPTION: N<>"${var}/path" expands ${...} inside the redirect target itself
# EXPECT_OUTPUT: roundtrip
# EXPECT_EXIT: 0
: "${TEST_TMPDIR:?TEST_TMPDIR not set}"
echo roundtrip 1<>"${TEST_TMPDIR}/rw_pe_direct"
cat "${TEST_TMPDIR}/rw_pe_direct"
