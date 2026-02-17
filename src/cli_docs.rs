use serde::Serialize;

#[derive(Serialize)]
pub struct CliDoc {
    pub name: String,
    pub version: Option<String>,
    pub about: Option<String>,
    pub global_options: Vec<OptionDoc>,
    pub commands: Vec<CommandDoc>,
}

#[derive(Serialize)]
pub struct CommandDoc {
    pub name: String,
    pub full_command: String,
    pub about: Option<String>,
    pub usage: String,
    pub options: Vec<OptionDoc>,
    pub positional_args: Vec<ArgDoc>,
    pub subcommands: Vec<CommandDoc>,
}

#[derive(Serialize)]
pub struct OptionDoc {
    pub name: String,
    pub short: Option<String>,
    pub long: Option<String>,
    pub help: Option<String>,
    pub required: bool,
    pub takes_value: bool,
    pub default_value: Option<String>,
}

#[derive(Serialize)]
pub struct ArgDoc {
    pub name: String,
    pub help: Option<String>,
    pub required: bool,
    pub default_value: Option<String>,
}

fn extract_option(arg: &clap::Arg) -> OptionDoc {
    OptionDoc {
        name: arg.get_id().to_string(),
        short: arg.get_short().map(|c| format!("-{}", c)),
        long: arg.get_long().map(|s| format!("--{}", s)),
        help: arg.get_help().map(|s| s.to_string()),
        required: arg.is_required_set(),
        takes_value: arg
            .get_num_args()
            .map(|n| n.takes_values())
            .unwrap_or(false),
        default_value: arg
            .get_default_values()
            .first()
            .map(|s| s.to_string_lossy().to_string()),
    }
}

fn extract_command(cmd: &mut clap::Command, parent_path: &str) -> CommandDoc {
    let full_command = if parent_path.is_empty() {
        cmd.get_name().to_string()
    } else {
        format!("{} {}", parent_path, cmd.get_name())
    };

    // Set bin_name so usage shows full command path
    *cmd = cmd.clone().bin_name(&full_command);

    let (options, positional): (Vec<_>, Vec<_>) = cmd
        .get_arguments()
        .filter(|a| a.get_id() != "help" && a.get_id() != "version")
        .partition(|a| !a.is_positional());

    let options: Vec<OptionDoc> = options.into_iter().map(extract_option).collect();
    let positional_args: Vec<ArgDoc> = positional
        .into_iter()
        .map(|a| ArgDoc {
            name: a.get_id().to_string(),
            help: a.get_help().map(|s| s.to_string()),
            required: a.is_required_set(),
            default_value: a
                .get_default_values()
                .first()
                .map(|s| s.to_string_lossy().to_string()),
        })
        .collect();

    let usage = cmd.render_usage().to_string();

    let subcommands: Vec<CommandDoc> = cmd
        .get_subcommands_mut()
        .filter(|s| s.get_name() != "help")
        .map(|s| extract_command(s, &full_command))
        .collect();

    CommandDoc {
        name: cmd.get_name().to_string(),
        full_command,
        about: cmd.get_about().map(|s| s.to_string()),
        usage,
        options,
        positional_args,
        subcommands,
    }
}

pub fn generate_json(cmd: &mut clap::Command) -> String {
    let bin_name = cmd.get_bin_name().unwrap_or(cmd.get_name()).to_string();

    let global_options: Vec<OptionDoc> = cmd
        .get_arguments()
        .filter(|a| a.get_id() != "help" && a.get_id() != "version")
        .map(extract_option)
        .collect();

    let commands: Vec<CommandDoc> = cmd
        .get_subcommands_mut()
        .filter(|s| s.get_name() != "help")
        .map(|s| extract_command(s, &bin_name))
        .collect();

    let doc = CliDoc {
        name: cmd.get_name().to_string(),
        version: cmd.get_version().map(|s| s.to_string()),
        about: cmd.get_about().map(|s| s.to_string()),
        global_options,
        commands,
    };

    serde_json::to_string_pretty(&doc).expect("valid json")
}

pub fn generate_markdown(cmd: &mut clap::Command) -> String {
    let bin_name = cmd.get_bin_name().unwrap_or(cmd.get_name()).to_string();
    let mut out = String::new();

    // Header
    out.push_str(&format!("# {} CLI Reference\n\n", cmd.get_name()));
    if let Some(about) = cmd.get_about() {
        out.push_str(&format!("> {}\n\n", about));
    }
    if let Some(version) = cmd.get_version() {
        out.push_str(&format!("**Version:** {}\n\n", version));
    }

    // Table of contents
    out.push_str("## Table of Contents\n\n");
    fn build_toc(cmd: &clap::Command, out: &mut String, parent_path: &str, depth: usize) {
        for sub in cmd.get_subcommands() {
            if sub.get_name() == "help" {
                continue;
            }
            let full_path = format!("{} {}", parent_path, sub.get_name());
            let anchor = full_path.replace(' ', "-");
            let indent = "  ".repeat(depth);
            out.push_str(&format!("{}- [`{}`](#{})\n", indent, full_path, anchor));
            build_toc(sub, out, &full_path, depth + 1);
        }
    }
    build_toc(cmd, &mut out, &bin_name, 0);
    out.push('\n');

    // Global options
    out.push_str("## Global Options\n\n");
    out.push_str("These options apply to all commands:\n\n");
    out.push_str("```\n");
    out.push_str(&cmd.render_help().to_string());
    out.push_str("\n```\n\n");
    out.push_str("---\n\n");

    // Commands
    out.push_str("## Commands\n\n");

    fn render_command(cmd: &mut clap::Command, out: &mut String, depth: usize, parent: &str) {
        for sub in cmd.get_subcommands_mut() {
            if sub.get_name() == "help" {
                continue;
            }

            let full_name = if parent.is_empty() {
                sub.get_name().to_string()
            } else {
                format!("{} {}", parent, sub.get_name())
            };

            // Set bin_name so usage shows full command path
            *sub = sub.clone().bin_name(&full_name);

            let anchor = full_name.replace(' ', "-");
            let heading = "#".repeat((depth + 3).min(6));

            out.push_str(&format!("{} `{}` {{#{}}}\n\n", heading, full_name, anchor));

            if let Some(about) = sub.get_about() {
                out.push_str(&format!("{}\n\n", about));
            }

            out.push_str("```\n");
            out.push_str(&sub.render_help().to_string());
            out.push_str("\n```\n\n");

            // Recurse into subcommands
            if sub.get_subcommands().any(|s| s.get_name() != "help") {
                render_command(sub, out, depth + 1, &full_name);
            }
        }
    }

    render_command(cmd, &mut out, 0, &bin_name);

    out.push_str("---\n\n");
    out.push_str(&format!(
        "*This documentation was auto-generated by `{} docs cli`*\n",
        bin_name
    ));

    out
}

pub fn generate_commands_list(cmd: &clap::Command, with_desc: bool) -> String {
    let bin_name = cmd.get_bin_name().unwrap_or(cmd.get_name()).to_string();
    let mut lines = Vec::new();

    fn collect_commands(
        cmd: &clap::Command,
        parent_path: &str,
        lines: &mut Vec<(String, Option<String>)>,
    ) {
        for sub in cmd.get_subcommands() {
            if sub.get_name() == "help" {
                continue;
            }

            let full_path = if parent_path.is_empty() {
                sub.get_name().to_string()
            } else {
                format!("{} {}", parent_path, sub.get_name())
            };

            let about = sub.get_about().map(|s| s.to_string());

            // Check if this command has subcommands (other than help)
            let has_subcommands = sub.get_subcommands().any(|s| s.get_name() != "help");

            if has_subcommands {
                // Recurse into subcommands
                collect_commands(sub, &full_path, lines);
            } else {
                // Leaf command - add it
                lines.push((full_path, about));
            }
        }
    }

    collect_commands(cmd, &bin_name, &mut lines);

    if with_desc {
        lines
            .into_iter()
            .map(|(cmd, desc)| {
                if let Some(d) = desc {
                    format!("{} # {}", cmd, d)
                } else {
                    cmd
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        lines
            .into_iter()
            .map(|(cmd, _)| cmd)
            .collect::<Vec<_>>()
            .join("\n")
    }
}
