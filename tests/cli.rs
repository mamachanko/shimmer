use std::process::Command;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_shimmer"))
}

#[test]
fn version_and_help_need_no_terminal() {
    let version = binary().arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap(),
        format!(
            "shimmer {}\n",
            option_env!("SHIMMER_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
        )
    );
    let help = binary().arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("<assemble|shimmer> <expanded-prompt>")
    );
}

#[test]
fn invalid_arguments_do_not_emit_terminal_controls() {
    for args in [
        vec![],
        vec!["assemble"],
        vec!["unknown", "prompt"],
        vec!["assemble", "prompt", "extra"],
    ] {
        let output = binary().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.contains(&0x1b));
    }
}

#[test]
fn literal_prompt_starting_with_dash_is_data_and_non_tty_is_rejected() {
    let output = binary()
        .args(["assemble", "--help\n\x1b[31m$ "])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "shimmer: stdout is not a terminal\n"
    );
}
