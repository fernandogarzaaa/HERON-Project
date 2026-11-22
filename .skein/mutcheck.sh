#!/bin/sh
# Apply each F5 mutant by hand, run its killing test, restore the file.
cd "$(dirname "$0")/worktrees/F5mut" || exit 1
f=src/tools/shortcuts.rs
cp $f "$TMPDIR/orig.rs"
try() {
  sed -i '' -e "$2" $f
  if cmp -s $f "$TMPDIR/orig.rs"; then
    echo "$1: MUTATION NOT APPLIED"
  else
    cargo test --locked --lib "tools::shortcuts::tests::$3" 2>&1 | grep -E "^test result" | sed "s/^/$1: /"
  fi
  cp "$TMPDIR/orig.rs" $f
}
try L46 '46s/1024 \* 1024/1024 + 1024/' child_output_is_kept_up_to_one_mebibyte
try L97 '97s/e.kind() == std::io::ErrorKind::NotFound/true/' a_binary_that_cannot_start_is_not_a_missing_binary
try L151 '151s/e.kind() == std::io::ErrorKind::NotFound/true/' an_unreadable_output_file_is_an_error_not_an_empty_result
try L258 '258s/"Run one of.*"$/"xyzzy"/' the_description_tells_the_model_which_shortcuts_can_run
cmp $f "$TMPDIR/orig.rs" && echo restored
