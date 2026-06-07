use assert_cmd::Command;
use serde_json::Value;
use std::env;
use std::fs;
use std::path::Path;

#[test]
#[ignore = "requires BBDOWN_LIVE_URL and optional live credentials"]
fn live_info_json_for_sample_url() -> anyhow::Result<()> {
    let Some(url) = live_var("BBDOWN_LIVE_URL") else {
        eprintln!("skipping live info test: BBDOWN_LIVE_URL is not set");
        return Ok(());
    };
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    write_live_credentials(&credential_file)?;

    let mut command = live_command(&credential_file)?;
    command.args(["info", &url, "--json"]);
    add_live_selection(&mut command);
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert!(json.get("video").is_some() || json.get("season").is_some());
    Ok(())
}

#[test]
#[ignore = "requires BBDOWN_LIVE_URL and optional live credentials"]
fn live_plan_json_for_sample_url() -> anyhow::Result<()> {
    let Some(url) = live_var("BBDOWN_LIVE_URL") else {
        eprintln!("skipping live plan test: BBDOWN_LIVE_URL is not set");
        return Ok(());
    };
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    write_live_credentials(&credential_file)?;

    let mut command = live_command(&credential_file)?;
    command.args(["plan", &url, "--json"]);
    add_live_selection(&mut command);
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert!(
        json["entries"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty())
    );
    Ok(())
}

fn live_command(credential_file: &Path) -> anyhow::Result<Command> {
    let mut command = Command::cargo_bin("bbdown")?;
    command.arg("--credential-file").arg(credential_file);
    Ok(command)
}

fn add_live_selection(command: &mut Command) {
    if let Some(selection) = live_var("BBDOWN_LIVE_SELECTION") {
        command.args(["--select", &selection]);
    }
}

fn write_live_credentials(path: &Path) -> anyhow::Result<()> {
    let credentials = serde_json::json!({
        "cookie": live_var("BBDOWN_LIVE_COOKIE"),
        "access_key": live_var("BBDOWN_LIVE_ACCESS_KEY"),
    });
    fs::write(path, serde_json::to_vec_pretty(&credentials)?)?;
    Ok(())
}

fn live_var(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}
