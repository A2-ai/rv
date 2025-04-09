wd <- tinytest::get_call_wd()
lib_loc <- file.path(wd, lib_loc)
suppressMessages(library(rv.git.pkgA, lib.loc = lib_loc))
# this message is only availabe at this branch
expect_equal(what_version_am_i(), "pkgA - v2 - branch: test-branch")