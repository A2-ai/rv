wd <- tinytest::get_call_wd()
lib_loc <- file.path(wd, lib_loc)
expect_equal(toString(packageVersion("pmplots", lib.loc = lib_loc)), "0.5.1")
expect_equal(toString(packageVersion("ggplot2", lib.loc = lib_loc)), "3.5.1")