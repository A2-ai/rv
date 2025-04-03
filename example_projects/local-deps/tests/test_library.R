wd <- tinytest::get_call_wd()
lib_loc <- file.path(wd, lib_loc)
expect_silent(library("dummy", lib.loc =  lib_loc))