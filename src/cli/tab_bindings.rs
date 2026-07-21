use clap::{Arg, Command};

use crate::api::schema::tabs::{
    tab_id, TabCreateCommandParams, TabCreateProjection, TabListProjection, TabRenameProjection,
};
use crate::api::schema::{Method, Request, TabTarget};

use super::super::projection::{self, cli_projection, Field, Leaf, ParseError};

cli_projection! {
    pub(crate) TabTargetProjection for TabTarget {
        tab_id => Field::new(String::new()).input(tab_id()),
    }
}

fn tab_create_arguments() -> Vec<Arg> {
    let mut arguments = TabCreateProjection::arguments();
    arguments.push(
        Arg::new("argv")
            .value_name("ARGV")
            .num_args(0..)
            .last(true)
            .help(
                "Command the tab's root pane runs directly instead of the configured interactive shell; the tab closes when it exits",
            ),
    );
    arguments
}

const LEAVES: [Leaf; 6] = [
    Leaf {
        name: "list",
        about: "List tabs",
        id: "cli:tab:list",
        arguments: TabListProjection::arguments,
        method: |matches| TabListProjection::from_matches(matches).map(Method::TabList),
    },
    Leaf {
        name: "create",
        about: "Create a tab",
        id: "cli:tab:create",
        arguments: tab_create_arguments,
        method: |matches| TabCreateProjection::from_matches(matches).map(Method::TabCreate),
    },
    Leaf {
        name: "get",
        about: "Show a tab",
        id: "cli:tab:get",
        arguments: TabTargetProjection::arguments,
        method: |matches| TabTargetProjection::from_matches(matches).map(Method::TabGet),
    },
    Leaf {
        name: "focus",
        about: "Focus a tab",
        id: "cli:tab:focus",
        arguments: TabTargetProjection::arguments,
        method: |matches| TabTargetProjection::from_matches(matches).map(Method::TabFocus),
    },
    Leaf {
        name: "rename",
        about: "Rename a tab",
        id: "cli:tab:rename",
        arguments: TabRenameProjection::arguments,
        method: |matches| TabRenameProjection::from_matches(matches).map(Method::TabRename),
    },
    Leaf {
        name: "close",
        about: "Close a tab",
        id: "cli:tab:close",
        arguments: TabTargetProjection::arguments,
        method: |matches| TabTargetProjection::from_matches(matches).map(Method::TabClose),
    },
];

pub(crate) fn command() -> Command {
    Command::new("tab")
        .about("Manage tabs over the socket API")
        .subcommand_required(true)
        .subcommands(LEAVES.iter().map(Leaf::command))
}

pub(crate) fn request(args: &[String]) -> Result<Request, ParseError> {
    let Request { id, method } = projection::group_request(command(), &LEAVES, args)?;
    // `tab create -- <argv>` sends a separate `tab.create_command` method rather than
    // a field on `tab.create`, so servers without support reject it instead of
    // silently opening a shell tab. Clap consumes the trailing argv for grammar and
    // help, but the values are read back from argv here rather than from matches.
    let method = match method {
        Method::TabCreate(tab) => match args.iter().position(|arg| arg == "--") {
            Some(separator) => {
                let argv = args[separator + 1..].to_vec();
                if argv.is_empty() {
                    return Err(ParseError::Usage("missing command after --".into()));
                }
                Method::TabCreateCommand(TabCreateCommandParams { tab, argv })
            }
            None => Method::TabCreate(tab),
        },
        other => other,
    };
    Ok(Request { id, method })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Request, ParseError> {
        request(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn tab_projection_preserves_requests_defaults_and_maps() {
        for (args, expected) in [
            (
                vec!["list"],
                serde_json::json!({"id":"cli:tab:list","method":"tab.list","params":{}}),
            ),
            (
                vec!["create"],
                serde_json::json!({"id":"cli:tab:create","method":"tab.create","params":{"focus":false}}),
            ),
            (
                vec![
                    "create",
                    "--workspace",
                    "2",
                    "--cwd",
                    "relative",
                    "--label",
                    "first",
                    "--label",
                    "last",
                    "--focus",
                    "--no-focus",
                    "--env",
                    "K=first",
                    "--env",
                    "K=",
                    "--env",
                    "V=a=b",
                ],
                serde_json::json!({"id":"cli:tab:create","method":"tab.create","params":{"workspace_id":"2","cwd":"relative","label":"last","focus":false,"env":{"K":"","V":"a=b"}}}),
            ),
            (
                vec!["rename", "2", "two", "--words", "here"],
                serde_json::json!({"id":"cli:tab:rename","method":"tab.rename","params":{"tab_id":"2","label":"two --words here"}}),
            ),
            (
                vec!["get", "2"],
                serde_json::json!({"id":"cli:tab:get","method":"tab.get","params":{"tab_id":"2"}}),
            ),
            (
                vec!["focus", "2"],
                serde_json::json!({"id":"cli:tab:focus","method":"tab.focus","params":{"tab_id":"2"}}),
            ),
            (
                vec!["close", "tab-2"],
                serde_json::json!({"id":"cli:tab:close","method":"tab.close","params":{"tab_id":"tab-2"}}),
            ),
        ] {
            assert_eq!(
                serde_json::to_value(parse(&args).unwrap()).unwrap(),
                expected,
                "{args:?}"
            );
        }
        assert!(
            matches!(parse(&["create", "--env", "=value"]), Err(ParseError::Usage(message)) if message == "env key must not be empty")
        );
    }

    #[test]
    fn tab_projection_clap_grammar_and_ordered_focus() {
        let value = serde_json::to_value(
            parse(&[
                "create",
                "--focus",
                "--no-focus",
                "--focus",
                "--cwd=relative",
            ])
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            value["params"],
            serde_json::json!({"cwd":"relative", "focus":true})
        );
        assert!(
            matches!(parse(&["get", "2", "--help"]), Err(ParseError::Clap(error)) if error.exit_code() == 0)
        );
        for args in [
            vec!["get"],
            vec!["get", "2", "extra"],
            vec!["create", "--cwd"],
        ] {
            assert!(matches!(parse(&args), Err(ParseError::Clap(error)) if error.exit_code() == 2));
        }
        command().debug_assert();
    }
}
