wd <- tinytest::get_call_wd()
lib_loc <- file.path(wd, lib_loc)
suppressMessages(library(rv.git.pkgB, lib.loc = lib_loc))
# "v4" is only accessible at the exact commit/tag for pkgA and pkgB
# tag v4.0 of pkgB has an explicit remote dependency of v4.0 for pkgA
expect_equal(what_version_am_i(), "pkgB - v4\ndependencies: pkgA - v4")