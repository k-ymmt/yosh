#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands (Command Search and Execution)
# DESCRIPTION: PATH=dir prefix assignment on external command affects search for that command
# EXPECT_OUTPUT<<END
# hello_from_prefix_path
# still_works
# END
# EXPECT_EXIT: 0
printf '#!/bin/sh\necho hello_from_prefix_path\n' > "$TEST_TMPDIR/mytool"
chmod 755 "$TEST_TMPDIR/mytool"

# Prefix PATH override must be honored for this command's own search.
PATH="$TEST_TMPDIR" mytool

# The shell's own PATH must be unaffected afterward.
/bin/echo still_works >/dev/null
echo still_works
