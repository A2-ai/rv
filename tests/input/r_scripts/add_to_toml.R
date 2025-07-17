# Read the TOML file
toml_content <- readLines("rproject.toml")

# Find the dependencies section
dep_start <- grep("^dependencies = \\[$", toml_content)
dep_end <- grep("^\\]$", toml_content)

# Find the correct closing bracket for dependencies
# (the first ] after the dependencies = [ line)
dep_close <- dep_end[dep_end > dep_start][1]

# Check if dependencies array is empty or has items
if (dep_close == dep_start + 1) {
    # Empty dependencies array, add readr as first item
    new_content <- c(
        toml_content[1:dep_start],
        '\t"readr",',
        toml_content[dep_close:length(toml_content)]
    )
} else {
    # Has existing dependencies, add readr to the end
    new_content <- c(
        toml_content[1:(dep_close - 1)],
        '\t"readr",',
        toml_content[dep_close:length(toml_content)]
    )
}

# Write back to file
writeLines(new_content, "rproject.toml")

# Print confirmation
cat("Added 'readr' to dependencies in rproject.toml\n")
