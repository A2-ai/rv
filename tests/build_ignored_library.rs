//! Exercise R's preparation-before-exclusion ordering with real build hooks.
#![cfg(unix)]

use std::{collections::HashMap, io::Read, os::unix::fs::PermissionsExt, sync::Arc};

use fs_err as fs;
use rv::{Cancellation, RCmd, RInstall};

#[test]
#[ignore = "requires an R installation; set RV_R_BIN to select it"]
fn ignored_build_inputs_remain_available_to_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("pkg");
    let library = root.path().join("library");
    let output = root.path().join("output");
    for dir in [
        "pkg/R",
        "pkg/tools",
        "pkg/rv/scripts",
        "pkg/rv/library",
        "library",
        "output",
    ] {
        fs::create_dir_all(root.path().join(dir)).unwrap();
    }
    fs::write(source.join("DESCRIPTION"), "Package: rvbuildinput\nVersion: 0.0.1\nTitle: Build Input Test\nDescription: Test preservation of build inputs.\nAuthors@R: person('Test', 'Author', email='test@example.com', role=c('aut','cre'))\nLicense: GPL-3\n").unwrap();
    fs::write(source.join("NAMESPACE"), "export(answer)\n").unwrap();
    fs::write(source.join(".Rbuildignore"), "^rv$\n^tools$\n").unwrap();
    fs::write(source.join("tools/input"), "answer <- function() 42L\n").unwrap();
    fs::write(source.join("rv/scripts/helper"), "build helper\n").unwrap();
    fs::write(source.join("rv/library/unused"), "installed dependency\n").unwrap();
    fs::write(
        source.join("cleanup"),
        "#!/bin/sh\nset -eu\ntest -f rv/scripts/helper\ncp tools/input R/generated.R\n",
    )
    .unwrap();
    fs::set_permissions(
        source.join("cleanup"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let r = RInstall::default_from_given_path(
        std::env::var_os("RV_R_BIN").unwrap_or_else(|| "R".into()),
    )
    .expect("R must be installed for this integration test");
    let tarball = r
        .build(
            &source,
            None::<&std::path::Path>,
            &output,
            &[library],
            Arc::new(Cancellation::default()),
            &HashMap::new(),
            None,
        )
        .unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(
        fs::File::open(tarball).unwrap(),
    ));
    let mut generated = None;
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().into_owned();
        assert!(!path.starts_with("rvbuildinput/rv"));
        assert!(!path.starts_with("rvbuildinput/tools"));
        if path.ends_with("R/generated.R") {
            let mut contents = String::new();
            entry.read_to_string(&mut contents).unwrap();
            generated = Some(contents);
        }
    }
    assert_eq!(generated.as_deref(), Some("answer <- function() 42L\n"));
    assert!(!source.join("R/generated.R").exists());
    assert!(source.join("rv/library/unused").is_file());
}
