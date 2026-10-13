use clap::{Arg, ArgAction, Command};
use scry::cli::setup::{check_collisions, ArgCollisionError};

// ---------------------------------------------------------------------------------------------- //

#[test]
fn assembled_arguments_retain_duplicate_ids_and_flag_spellings() {
    let command = Command::new("test")
        .arg(Arg::new("input").long("input"))
        .arg(Arg::new("input").long("other"));
    assert_eq!(command.get_arguments().count(), 2);
    assert!(matches!(check_collisions(&command), Err(ArgCollisionError::Id(id)) if id == "input"));

    let command = Command::new("test")
        .arg(Arg::new("first").long("input"))
        .arg(Arg::new("second").long("input"));
    assert!(
        matches!(check_collisions(&command), Err(ArgCollisionError::Long(long)) if long == "input")
    );

    let command =
        Command::new("test").arg(Arg::new("first").short('i')).arg(Arg::new("second").short('i'));
    assert!(matches!(check_collisions(&command), Err(ArgCollisionError::Short('i'))));
}

#[test]
fn aliases_share_the_same_option_names_as_primary_spellings() {
    for visible in [false, true] {
        let first = Arg::new("first").long("input");
        let first = if visible {
            first.visible_alias("source").visible_short_alias('i')
        } else {
            first.alias("source").short_alias('i')
        };
        let command =
            Command::new("test").arg(first.clone()).arg(Arg::new("second").long("source"));
        assert!(
            matches!(check_collisions(&command), Err(ArgCollisionError::Long(long)) if long == "source")
        );

        let command = Command::new("test").arg(first).arg(Arg::new("second").short('i'));
        assert!(matches!(check_collisions(&command), Err(ArgCollisionError::Short('i'))));
    }
}

#[test]
fn enabled_help_is_reserved_and_disabled_help_can_be_defined_explicitly() {
    let command = Command::new("test").arg(Arg::new("help").long("manual"));
    assert!(matches!(check_collisions(&command), Err(ArgCollisionError::Id(id)) if id == "help"));

    let command = Command::new("test").arg(Arg::new("manual").long("help"));
    assert!(
        matches!(check_collisions(&command), Err(ArgCollisionError::Long(long)) if long == "help")
    );

    let command = Command::new("test").arg(Arg::new("manual").short('h'));
    assert!(matches!(check_collisions(&command), Err(ArgCollisionError::Short('h'))));

    let command = Command::new("test")
        .disable_help_flag(true)
        .arg(Arg::new("help").long("help").short('h').action(ArgAction::Help));
    check_collisions(&command).unwrap();
    command.debug_assert();
}

#[test]
fn version_names_are_reserved_only_when_clap_will_add_the_flag() {
    let argument = Arg::new("app-version").long("version").short('V');
    let command = Command::new("test").arg(argument.clone());
    check_collisions(&command).unwrap();
    command.debug_assert();

    let command = Command::new("test").version("1").arg(argument.clone());
    assert!(
        matches!(check_collisions(&command), Err(ArgCollisionError::Long(long)) if long == "version")
    );

    let command = Command::new("test").version("1").disable_version_flag(true).arg(argument);
    check_collisions(&command).unwrap();
    command.debug_assert();
}
