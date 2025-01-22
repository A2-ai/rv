pub const PACKAGE_FILENAME: &str = "PACKAGES";
pub const SOURCE_PACKAGES_PATH: &str = "/src/contrib/PACKAGES";
pub const LOCKFILE_NAME: &str = "rv.lock";
// List obtained from REPL: `rownames(installed.packages(priority="recommended"))`
pub const RECOMMENDED_PACKAGES: [&str; 15] = [
    "boot", 
    "class", 
    "cluster", 
    "codetools", 
    "foreign", 
    "KernSmooth", 
    "lattice", 
    "MASS", 
    "Matrix", 
    "mgcv", 
    "nlme", 
    "nnet", 
    "rpart", 
    "spatial", 
    "survival"
];
// List obtained from the REPL: `rownames(installed.packages(priority="base"))`
pub const BASE_PACKAGES: [&str; 14] = [
    "base",
    "compiler",
    "datasets",
    "grDevices",
    "graphics",
    "grid",
    "methods",
    "parallel",
    "splines",
    "stats",
    "stats4",
    "tcltk",
    "tools",
    "utils",
];
