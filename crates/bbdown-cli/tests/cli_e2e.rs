use assert_cmd::Command;
use httpmock::MockServer;
use httpmock::prelude::*;
use serde_json::Value;

#[test]
fn info_json_resolves_mock_video() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
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
        .arg("--credential-file")
        .arg(&credential_file)
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
fn plan_json_resolves_mock_video_streams() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
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
                "pages": [{"page": 1, "cid": 2, "part": "Main"}]
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/tag/archive/tags")
            .query_param("aid", "170001");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": []
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/player/playurl")
            .query_param("avid", "170001")
            .query_param("cid", "2")
            .query_param("try_look", "1");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "dash": {
                    "duration": 3,
                    "video": [{
                        "id": 80,
                        "baseUrl": "https://video.example/80.m4s",
                        "base_url": "https://video.example/80.m4s"
                    }],
                    "audio": [{
                        "id": 30280,
                        "baseUrl": "https://audio.example/30280.m4s",
                        "base_url": "https://audio.example/30280.m4s"
                    }]
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/player/v2")
            .query_param("aid", "170001")
            .query_param("cid", "2");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {"subtitle": {"subtitles": []}}
        }));
    });

    let mut command = Command::cargo_bin("bbdown")?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("plan")
        .arg("av170001")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(json["title"], "Mock video");
    assert_eq!(json["entries"][0]["streams"]["videos"][0]["id"], 80);
    assert_eq!(
        json["entries"][0]["danmaku"]["xml_url"],
        "https://comment.bilibili.com/2.xml"
    );
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
