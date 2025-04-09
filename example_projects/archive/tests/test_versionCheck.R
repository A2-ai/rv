# Test that the version of R6 is correct coming from the archive
wd <- tinytest::get_call_wd()
expect_equal(toString(packageVersion("R6", lib.loc = file.path(wd, lib_loc))), "2.5.0")
