#' @export
hello_location <- function(location = pkg:::world()) {
  print(paste("hello", location))
}
