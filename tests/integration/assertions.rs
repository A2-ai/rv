use crate::integration::{StringOrList, TestAssertion};
use anyhow::Result;

pub fn check_assertion(assertion: &TestAssertion, output: &str, _unused_stderr: &str) -> Result<()> {
    match assertion {
        TestAssertion::Single(expected) => {
            check_contains_assertion(expected, output)
        }
        TestAssertion::Multiple(expected_list) => {
            for expected in expected_list.iter() {
                check_contains_assertion(expected, output)?;
            }
            Ok(())
        }
        TestAssertion::Structured(structured) => {
            // Check positive assertions (contains)
            if let Some(contains) = &structured.contains {
                check_string_or_list_contains(contains, output)?;
            }

            // Check negative assertions (not-contains)
            if let Some(not_contains) = &structured.not_contains {
                check_string_or_list_not_contains(not_contains, output)?;
            }

            Ok(())
        }
    }
}

// Simplified assertion function that checks unified output
fn check_contains_assertion(expected: &str, output: &str) -> Result<()> {
    if output.contains(expected) {
        return Ok(());
    }

    // Not found - provide detailed error message
    return Err(anyhow::anyhow!(
        "Assertion failed: expected '{}' in output.\\n\\nOUTPUT ({} chars):\\n{}\\n\\nSearching for lines containing '{}':\\nMatches:\\n{}",
        expected,
        output.len(),
        output,
        expected.split(':').next().unwrap_or(expected),
        output
            .lines()
            .filter(|line| line.contains(expected.split(':').next().unwrap_or(expected)))
            .collect::<Vec<_>>()
            .join("\\n")
    ));
}

fn check_string_or_list_contains(
    contains: &StringOrList,
    output: &str,
) -> Result<()> {
    match contains {
        StringOrList::Single(expected) => {
            check_contains_assertion(expected, output)
        }
        StringOrList::Multiple(expected_list) => {
            for expected in expected_list.iter() {
                check_contains_assertion(expected, output)?;
            }
            Ok(())
        }
    }
}

fn check_string_or_list_not_contains(
    not_contains: &StringOrList,
    output: &str,
) -> Result<()> {
    match not_contains {
        StringOrList::Single(expected) => {
            // For negative assertions, fail if found in output
            if output.contains(expected) {
                return Err(anyhow::anyhow!(
                    "Negative assertion failed: found '{}' in output (expected NOT to find it).\\n\\nOUTPUT ({} chars):\\n{}",
                    expected,
                    output.len(),
                    output
                ));
            }
            Ok(())
        }
        StringOrList::Multiple(expected_list) => {
            for expected in expected_list.iter() {
                if output.contains(expected) {
                    return Err(anyhow::anyhow!(
                        "Negative assertion failed: found '{}' in output (expected NOT to find it).\\n\\nOUTPUT ({} chars):\\n{}",
                        expected,
                        output.len(),
                        output
                    ));
                }
            }
            Ok(())
        }
    }
}

pub fn filter_timing_from_output(output: &str) -> String {
    // Replace timing patterns like "in 0ms", "in 1ms", etc. with "in Xms"
    // This regex should always compile, but handle gracefully if it doesn't
    match regex::Regex::new(r" in \d+ms") {
        Ok(re) => re.replace_all(output, " in Xms").to_string(),
        Err(_) => {
            // Fallback: return original output if regex fails to compile
            output.to_string()
        }
    }
}
