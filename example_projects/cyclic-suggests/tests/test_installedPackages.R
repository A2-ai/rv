pkg_burden <- toString(c("base64enc", "bitops", "bslib", "cachem", "cli", "commonmark",
    "digest", "evaluate", "fastmap", "fontawesome", "fs", "glue", "highr", "htmltools", 
    "jquerylib", "jsonlite", "knitr","lifecycle", "markdown", "memoise", "mime", "R6", 
    "rappdirs", "RCurl", "rlang", "rmarkdown", "sass", "tinytex", "xfun", "yaml"))

wd <- tinytest::get_call_wd()
lib_loc <- file.path(wd, lib_loc)

ip <- toString(row.names(as.data.frame(installed.packages(lib.loc = lib_loc))))
expect_equal(toString(ip), toString(pkg_burden))