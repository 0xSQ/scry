//! Tests inheritance of exposed named arguments by application subcommands.

use scry::clap::Command;
use scry::cli::setup::ExposeMap;
use scry::Desc;

// ---------------------------------------------------------------------------------------------- //

#[test]
fn global_named_exposures_share_values_before_and_after_subcommands() {
    let mut exposures = ExposeMap::new();
    exposures.option("value").long("select").global(true);
    exposures.flag("enabled", true).global(true);
    exposures.preset("preset").set("value", "preset").global(true);

    let command = exposures
        .augment(Command::new("application"), &Desc::default())
        .subcommand(Command::new("child"));

    for arguments in [
        [
            "application",
            "--select",
            "chosen",
            "--enabled",
            "--preset",
            "child",
        ],
        [
            "application",
            "child",
            "--select",
            "chosen",
            "--enabled",
            "--preset",
        ],
    ] {
        let matches = command.clone().try_get_matches_from(arguments).unwrap();
        let child_matches = matches.subcommand_matches("child").unwrap();

        for matches in [&matches, child_matches] {
            assert_eq!(matches.get_one::<String>("select").map(String::as_str), Some("chosen"));
            assert!(matches.get_flag("enabled"));
            assert!(matches.get_flag("preset"));
        }
    }
}

#[test]
fn ordinary_exposures_remain_scoped_to_the_defining_command() {
    let mut exposures = ExposeMap::new();
    exposures.option("value").long("select");
    let command = exposures
        .augment(Command::new("application"), &Desc::default())
        .subcommand(Command::new("child"));

    assert!(command
        .clone()
        .try_get_matches_from(["application", "--select", "chosen", "child"])
        .is_ok());
    let error =
        command.try_get_matches_from(["application", "child", "--select", "chosen"]).unwrap_err();
    assert_eq!(error.kind(), scry::clap::error::ErrorKind::UnknownArgument);
}

#[test]
#[should_panic(expected = "exposed positional 'value' cannot be global")]
fn positional_global_modifier_is_rejected() {
    ExposeMap::new().positional("value").global(true);
}

#[test]
#[should_panic(expected = "exposed positional 'value' cannot be global")]
fn positional_global_public_field_is_rejected_during_augmentation() {
    let mut exposures = ExposeMap::new();
    exposures.positional("value").global = true;
    exposures.augment(Command::new("application"), &Desc::default());
}
