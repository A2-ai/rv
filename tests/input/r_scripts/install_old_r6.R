options(repos = c(CRAN = "https://packagemanager.posit.co/cran/2025-01-01"))
install.packages("R6", quiet = TRUE)
detach("package:R6", unload=TRUE)
flush.console()