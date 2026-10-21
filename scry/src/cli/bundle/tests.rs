use super::*;
use std::convert::Infallible;

// ---------------------------------------------------------------------------------------------- //

#[test]
fn root_handlers_receive_the_registered_root_despite_renamed_invocation() {
    let root = Bundle::single_with_commands(
        Command::new("application").bin_name("installed-name"),
        |_, commands| {
            assert_eq!(names(commands), ["application"]);
            assert_eq!(commands[0].get_bin_name(), Some("installed-name"));
            assert!(commands[0].find_subcommand("group").is_some());
            Ok::<_, Infallible>(())
        },
    )
    .with_subcommands(vec![Bundle::group(
        "group",
        "Groups commands.",
        vec![Bundle::single(Command::new("child"), |_| Ok(()))],
    )]);

    root.run_from(["renamed-invocation"]).unwrap();
}

#[test]
fn nested_handlers_receive_the_canonical_command_chain() {
    let leaf =
        Bundle::single_with_commands(Command::new("leaf").alias("leaf-alias"), |_, commands| {
            assert_eq!(names(commands), ["application", "group", "leaf"]);
            assert!(commands[0].find_subcommand("sibling").is_some());
            assert!(commands[1].find_subcommand("leaf").is_some());
            Ok::<_, Infallible>(())
        });
    let group = Bundle::group_with(Command::new("group").alias("group-alias"), vec![leaf])
        .fallback(|_| Ok(()));
    let root = Bundle::single(Command::new("application"), |_| Ok(()))
        .with_subcommands(vec![group])
        .with_subcommands(vec![Bundle::single(Command::new("sibling"), |_| Ok(()))]);

    root.run_from(["renamed-invocation", "group-alias", "leaf-alias"]).unwrap();
}

#[test]
fn executable_parent_handlers_receive_their_own_chain_as_fallbacks() {
    let parent = Bundle::single_with_commands(Command::new("parent"), |_, commands| {
        assert_eq!(names(commands), ["application", "group", "parent"]);
        assert!(commands[2].find_subcommand("child").is_some());
        Ok::<_, Infallible>(())
    })
    .with_subcommands(vec![Bundle::single(Command::new("child"), |_| Ok(()))]);
    let root = Bundle::group(
        "application",
        "Runs commands.",
        vec![Bundle::group("group", "Groups commands.", vec![parent])],
    );

    root.run_from(["application", "group", "parent"]).unwrap();
}

#[test]
fn group_fallbacks_can_consume_captured_values() {
    let captured = "fallback result".to_owned();
    let root = Bundle::group(
        "application",
        "Runs commands.",
        vec![Bundle::single(Command::new("child"), |_| {
            Ok::<_, Infallible>(String::new())
        })],
    )
    .fallback(move |_| Ok(captured));

    assert_eq!(root.run_from(["application"]).unwrap(), "fallback result");
}

#[test]
fn raw_clap_subcommands_remain_owned_by_their_enclosing_handler() {
    let root = Bundle::single_with_commands(
        Command::new("application").subcommand(Command::new("native").alias("native-alias")),
        |matches, commands| {
            assert_eq!(matches.subcommand_name(), Some("native"));
            assert_eq!(names(commands), ["application"]);
            assert!(commands[0].find_subcommand("native").is_some());
            Ok::<_, Infallible>(())
        },
    );

    root.run_from(["application", "native-alias"]).unwrap();
}

fn names<'a>(commands: &'a [&Command]) -> Vec<&'a str> {
    commands.iter().map(|command| command.get_name()).collect()
}
