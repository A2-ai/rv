use taplo::formatter;

pub fn format_document(document: &str) -> String {
    formatter::format(document, taplo::formatter::Options::default())
}

// add a test with a basic rproject.toml
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_simple() {
        let config_file = "src/tests/formatting/simple.toml";
        let contents = std::fs::read_to_string(config_file).expect("Failed to read config file");
        insta::assert_snapshot!("fmt-simple", format_document(&contents));
    }

    #[test]
    fn fmt_complex() {
        let config_file = "src/tests/formatting/kitchen-sink.toml";
        let contents = std::fs::read_to_string(config_file).expect("Failed to read config file");
        insta::assert_snapshot!("fmt-complex", format_document(&contents));
    }
}
