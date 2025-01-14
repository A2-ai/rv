# Basic End to End test
The purpose of this test is to very basically ensure packages can be downloaded from multiple repositories, binaries properly unzipped, and source packages properly installed. We accomplish this by downloading a source package from a repository in which the package's dependencies are not in the same repository.

## Steps:

1. Install `scicalc` from repo "https://a2-ai.github.io/gh-pkg-mirror/scicalc/" with additional dependency repository "https://packagemanager.posit.co/cran/2024-12-04"
2. Verify `arrow`, `checkmate`, `digest`, `dplyr`, `fs`, `haven`, `magrittr`, `pzfx`, `readr`, `rlang`, `stats`, `stringr`, `assertthat`, `bit64`, `glue`, `methods`, `purrr`, `R6`, `tidyselect`, `utils`, `vctrs`, `cpp11`, `backports`, `cli`, `generics`, `lifecycle`, `pillar`, `tibble`, `forcats`, `hms`, `xml2`, `clipr`, `crayon`, `vroom`, `tzdb`, `stringi`, `tools`, `bit`, `grDevices`, `pkgconfig`, `fansi`, `utf8`, `withr`, `progress`, `prettyunits`, and `graphics` are installed and are not source folders

## Manual Testing Results:
```
$ RV_LINK_MODE=symlink cargo run --features=cli -- --config-file=e2e_tests/basic/rproject.toml -v sync

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
     Running `target/debug/rv --config-file=e2e_tests/basic/rproject.toml -v sync`
+ crayon (1.5.3) in 287ms
+ clipr (0.8.0) in 316ms
+ withr (3.0.2) in 332ms
+ stringi (1.8.4) in 657ms
+ prettyunits (1.2.0) in 375ms
+ cpp11 (0.5.0) in 431ms
+ rlang (1.1.4) in 575ms
+ assertthat (0.2.1) in 348ms
+ digest (0.6.37) in 420ms
+ glue (1.8.0) in 344ms
+ fs (1.6.5) in 407ms
+ bit (4.5.0.1) in 458ms
+ utf8 (1.2.4) in 396ms
+ cli (3.6.3) in 439ms
+ generics (0.1.3) in 328ms
+ R6 (2.5.1) in 400ms
+ backports (1.5.0) in 427ms
+ fansi (1.0.6) in 392ms
+ pkgconfig (2.0.3) in 332ms
+ magrittr (2.0.3) in 390ms
+ tzdb (0.4.0) in 406ms
+ bit64 (4.5.2) in 373ms
+ lifecycle (1.0.4) in 359ms
+ xml2 (1.3.6) in 333ms
+ checkmate (2.3.2) in 360ms
+ vctrs (0.6.5) in 371ms
+ pzfx (0.3.0) in 366ms
+ tidyselect (1.2.1) in 370ms
+ stringr (1.5.1) in 432ms
+ purrr (1.0.2) in 440ms
+ hms (1.1.3) in 267ms
+ pillar (1.9.0) in 306ms
+ progress (1.2.3) in 338ms
+ tibble (3.2.1) in 275ms
+ forcats (1.0.0) in 429ms
+ vroom (1.6.5) in 475ms
+ dplyr (1.1.4) in 498ms
+ readr (2.1.5) in 263ms
+ arrow (17.0.0.1) in 1512ms
+ haven (2.5.4) in 276ms
+ scicalc (0.0.0.9004) in 1218ms
```
total: ~8 secs

### Using R install.packages
```
> tictoc::tic("scicalc"); install.packages("scicalc", repos = c(scicalc = "https://a2-ai.github.io/gh-pkg-mirror/scicalc", rspm = "https://packagemanager.posit.co/cran/__linux__/jammy/2024-12-04")); tictoc::toc();

Installing package into ‘/cluster-data/user-homes/wes/R/x86_64-pc-linux-gnu-library/4.3’
(as ‘lib’ is unspecified)
also installing the dependencies ‘prettyunits’, ‘bit’, ‘withr’, ‘fansi’, ‘utf8’, ‘pkgconfig’, ‘progress’, ‘assertthat’, ‘bit64’, ‘glue’, ‘purrr’, ‘R6’, ‘tidyselect’, ‘vctrs’, ‘cpp11’, ‘backports’, ‘cli’, ‘generics’, ‘lifecycle’, ‘pillar’, ‘tibble’, ‘forcats’, ‘hms’, ‘xml2’, ‘clipr’, ‘crayon’, ‘vroom’, ‘tzdb’, ‘stringi’, ‘arrow’, ‘checkmate’, ‘digest’, ‘dplyr’, ‘fs’, ‘haven’, ‘magrittr’, ‘pzfx’, ‘readr’, ‘rlang’, ‘stringr’

...

scicalc: 30.357 sec elapsed
```
