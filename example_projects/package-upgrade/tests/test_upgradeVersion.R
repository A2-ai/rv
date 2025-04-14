wd <- tinytest::get_call_wd()
lib_loc <- file.path(wd, lib_loc)
.libPaths(lib_loc)
expect_equal(toString(packageVersion("pmplots")), "0.5.1")
expect_equal(toString(packageVersion("ggplot2")), "3.5.1")
# `rot_xy` is a new function exported in v0.5.0 of `pmplots`
# https://metrumresearchgroup.github.io/pmplots/news/index.html#pmplots-050
# Of the 4 repositories in the rproject.toml, a version >= 0.5.0 of `pmplots`
# is only available in the one specified
library("pmplots")
expect_true("rot_xy" %in% getNamespaceExports("pmplots"))