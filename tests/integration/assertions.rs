use crate::integration::{StringOrList, TestAssertion};
use anyhow::Result;

pub fn check_assertion(assertion: &TestAssertion, stdout: &str, stderr: &str) -> Result<()> {
    match assertion {
        TestAssertion::Single(expected) => {
            check_contains_assertion_combined(expected, stdout, stderr)
        }
        TestAssertion::Multiple(expected_list) => {
            for expected in expected_list.iter() {
                check_contains_assertion_combined(expected, stdout, stderr)?;
            }
            Ok(())
        }
        TestAssertion::Structured(structured) => {
            // Check positive assertions (contains)
            if let Some(contains) = &structured.contains {
                check_string_or_list_contains_combined(contains, stdout, stderr)?;
            }

            // Check negative assertions (not-contains)
            if let Some(not_contains) = &structured.not_contains {
                check_string_or_list_not_contains_combined(not_contains, stdout, stderr)?;
            }

            Ok(())
        }
    }
}

// Combined assertion functions that check stdout first, then stderr
fn check_contains_assertion_combined(expected: &str, stdout: &str, stderr: &str) -> Result<()> {
    // First try stdout
    if stdout.contains(expected) {
        return Ok(());
    }

    // Then try stderr
    if stderr.contains(expected) {
        return Ok(());
    }

    // Not found in either - provide detailed error message
    return Err(anyhow::anyhow!(
        "Assertion failed: expected '{}' in output.\\n\\nSTDOUT ({} chars):\\n{}\\n\\nSTDERR ({} chars):\\n{}\\n\\nSearching for lines containing '{}':\\nSTDOUT matches:\\n{}\\nSTDERR matches:\\n{}",
        expected,
        stdout.len(),
        stdout,
        stderr.len(),
        stderr,
        expected.split(':').next().unwrap_or(expected),
        stdout
            .lines()
            .filter(|line| line.contains(expected.split(':').next().unwrap_or(expected)))
            .collect::<Vec<_>>()
            .join("\\n"),
        stderr
            .lines()
            .filter(|line| line.contains(expected.split(':').next().unwrap_or(expected)))
            .collect::<Vec<_>>()
            .join("\\n")
    ));
}

fn check_string_or_list_contains_combined(
    contains: &StringOrList,
    stdout: &str,
    stderr: &str,
) -> Result<()> {
    match contains {
        StringOrList::Single(expected) => {
            check_contains_assertion_combined(expected, stdout, stderr)
        }
        StringOrList::Multiple(expected_list) => {
            for expected in expected_list.iter() {
                check_contains_assertion_combined(expected, stdout, stderr)?;
            }
            Ok(())
        }
    }
}

fn check_string_or_list_not_contains_combined(
    not_contains: &StringOrList,
    stdout: &str,
    stderr: &str,
) -> Result<()> {
    match not_contains {
        StringOrList::Single(expected) => {
            // For negative assertions, fail if found in either stdout OR stderr
            if stdout.contains(expected) || stderr.contains(expected) {
                return Err(anyhow::anyhow!(
                    "Negative assertion failed: found '{}' in output (expected NOT to find it).\\n\\nSTDOUT ({} chars):\\n{}\\n\\nSTDERR ({} chars):\\n{}",
                    expected,
                    stdout.len(),
                    stdout,
                    stderr.len(),
                    stderr
                ));
            }
            Ok(())
        }
        StringOrList::Multiple(expected_list) => {
            for expected in expected_list.iter() {
                if stdout.contains(expected) || stderr.contains(expected) {
                    return Err(anyhow::anyhow!(
                        "Negative assertion failed: found '{}' in output (expected NOT to find it).\\n\\nSTDOUT ({} chars):\\n{}\\n\\nSTDERR ({} chars):\\n{}",
                        expected,
                        stdout.len(),
                        stdout,
                        stderr.len(),
                        stderr
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
