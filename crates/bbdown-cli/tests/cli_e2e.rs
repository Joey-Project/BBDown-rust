use assert_cmd::Command;
use httpmock::MockServer;
use httpmock::prelude::*;
use serde_json::Value;

#[test]
fn info_json_resolves_mock_video() -> anyhow::Result<()> {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/web-interface/view")
            .query_param("aid", "170001");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "aid": 170_001,
                "bvid": "BV1xx411c7mD",
                "title": "Mock video",
                "desc": "Mock description",
                "owner": {"mid": 1, "name": "Tester"},
                "pages": [{"page": 1, "cid": 2, "part": "Main", "duration": 3}]
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/tag/archive/tags")
            .query_param("aid", "170001");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": [{"tag_id": 9, "tag_name": "mock"}]
        }));
    });

    let mut command = Command::cargo_bin("bbdown")?;
    command
        .arg("--api-base")
        .arg(server.base_url())
        .arg("info")
        .arg("av170001")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(json["video"]["title"], "Mock video");
    assert_eq!(json["video"]["tags"][0]["name"], "mock");
    Ok(())
}

#[test]
fn auth_import_status_and_logout_use_local_store() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");

    Command::cargo_bin("bbdown")?
        .arg("--credential-file")
        .arg(&credential_file)
        .env("BBDOWN_COOKIE", "SESSDATA=secret")
        .args(["auth", "import-cookie"])
        .assert()
        .success();

    let output = Command::cargo_bin("bbdown")?
        .arg("--credential-file")
        .arg(&credential_file)
        .args(["auth", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: Value = serde_json::from_slice(&output)?;
    assert_eq!(status["has_cookie"], true);

    Command::cargo_bin("bbdown")?
        .arg("--credential-file")
        .arg(&credential_file)
        .args(["auth", "logout"])
        .assert()
        .success();

    let output = Command::cargo_bin("bbdown")?
        .arg("--credential-file")
        .arg(&credential_file)
        .args(["auth", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status: Value = serde_json::from_slice(&output)?;
    assert_eq!(status["has_cookie"], false);
    Ok(())
}
