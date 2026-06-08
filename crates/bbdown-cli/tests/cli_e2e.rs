use assert_cmd::Command;
use bbdown::{CredentialStore, Credentials};
use httpmock::MockServer;
use httpmock::prelude::*;
use serde_json::Value;
use std::fs;
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const CLI_OVERRIDE_ENV_VARS: &[&str] = &[
    "BBDOWN_API_BASE",
    "BBDOWN_PGC_BASE",
    "BBDOWN_INTL_BASE",
    "BBDOWN_COMMENT_BASE",
    "BBDOWN_PASSPORT_BASE",
    "BBDOWN_TV_PASSPORT_BASE",
    "BBDOWN_TV_PASSPORT_POLL_BASE",
    "BBDOWN_RESTRICTED_AREA",
    "BBDOWN_RESTRICTED_AREA_PROXY",
    "BBDOWN_RESTRICTED_API_PROXY",
    "BBDOWN_CREDENTIAL_FILE",
    "BBDOWN_REQUEST_TIMEOUT_SECONDS",
    "BBDOWN_COOKIE",
    "BBDOWN_ACCESS_KEY",
];

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

    let mut command = bbdown_command()?;
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
                "accept_quality": [80, 64],
                "accept_description": ["1080P", "720P"],
                "support_formats": [
                    {"quality": 80, "new_description": "1080P 高码率"},
                    {"quality": 64, "display_desc": "720P"}
                ],
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

    let mut command = bbdown_command()?;
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
    assert_plan_stream_qualities(&json);
    assert_eq!(
        json["entries"][0]["danmaku"]["xml_url"],
        "https://comment.bilibili.com/2.xml"
    );

    assert_human_plan_lists_qualities(&credential_file, &server)?;
    Ok(())
}

fn assert_plan_stream_qualities(json: &Value) {
    assert_eq!(json["entries"][0]["streams"]["accept_quality"][0], 80);
    assert_eq!(json["entries"][0]["streams"]["accept_quality"][1], 64);
    assert_eq!(json["entries"][0]["streams"]["qualities"][0]["id"], 80);
    assert_eq!(
        json["entries"][0]["streams"]["qualities"][0]["description"],
        "1080P 高码率"
    );
    assert_eq!(
        json["entries"][0]["streams"]["qualities"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

fn assert_human_plan_lists_qualities(
    credential_file: &std::path::Path,
    server: &MockServer,
) -> anyhow::Result<()> {
    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("plan")
        .arg("av170001");
    let output = command.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(output)?;
    assert!(text.contains("qualities: 80 (1080P 高码率)"));
    assert!(!text.contains("64 (720P)"));
    assert!(text.contains("videos: 1"));
    assert!(text.contains("q=80"));
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn plan_json_uses_restricted_area_proxy_after_official_pgc_failure() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    CredentialStore::new(credential_file.clone()).save(&Credentials {
        cookie: Some("SESSDATA=COOKIE_SECRET".to_owned()),
        access_key: Some("ACCESS_SECRET".to_owned()),
        tv_access_key: None,
    })?;
    server.mock(|when, then| {
        when.method(GET)
            .path("/pgc/view/web/season")
            .query_param("ep_id", "1000");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "result": {
                "season_id": 123,
                "title": "Restricted Season",
                "episodes": [
                    {"aid": 10, "bvid": "BV1aa", "cid": 100, "id": 1000, "ep_id": 1000, "title": "1", "long_title": "Start"}
                ]
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/pgc/player/web/v2/playurl")
            .query_param("ep_id", "1000");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": -40301,
            "message": "area restricted"
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/proxy-playurl")
            .query_param("proxy_token", "a=b")
            .query_param("ep_id", "1000")
            .query_param("area", "hk")
            .query_param("access_key", "ACCESS_SECRET")
            .header_missing("cookie");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "timelength": 3000,
            "accept_quality": [80],
            "accept_description": ["1080P"],
            "support_formats": [{"quality": 80, "new_description": "1080P 高码率"}],
            "dash": {
                "duration": 3,
                "video": [{
                    "id": 80,
                    "baseUrl": "https://proxy.example/video.m4s",
                    "base_url": "https://proxy.example/video.m4s"
                }],
                "audio": []
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/player/v2")
            .query_param("aid", "10")
            .query_param("cid", "100");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {"subtitle": {"subtitles": []}}
        }));
    });

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("--pgc-base")
        .arg(server.base_url())
        .arg("--restricted-area")
        .arg("hk")
        .arg("--restricted-area-proxy")
        .arg(format!(
            "hk={}/proxy-playurl?proxy_token=a%3Db",
            server.base_url()
        ))
        .arg("plan")
        .arg("ep1000")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;

    assert_eq!(json["entries"][0]["source"], "pgc_proxy");
    assert_eq!(json["entries"][0]["streams"]["videos"][0]["id"], 80);
    assert_eq!(json["entries"][0]["streams"]["qualities"][0]["id"], 80);
    assert_eq!(
        json["entries"][0]["streams"]["qualities"][0]["description"],
        "1080P 高码率"
    );
    assert_eq!(
        json["entries"][0]["diagnostics"]["attempts"][0]["source"],
        "pgc_web"
    );
    assert_eq!(
        json["entries"][0]["diagnostics"]["attempts"][1]["area"],
        "hk"
    );
    let output_text = String::from_utf8(output)?;
    assert!(!output_text.contains("ACCESS_SECRET"));
    assert!(!output_text.contains("COOKIE_SECRET"));
    assert!(!output_text.contains("access_key"));
    assert!(!output_text.contains("proxy_token"));
    Ok(())
}

#[test]
#[allow(clippy::too_many_lines)]
fn download_json_writes_mock_media_files() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
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
            .path("/x/player/playurl")
            .query_param("avid", "170001")
            .query_param("cid", "2")
            .query_param("try_look", "1");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "dash": {
                    "duration": 3,
                    "video": [
                        {
                            "id": 80,
                            "baseUrl": format!("{}/video.m4s", server.base_url()),
                            "base_url": format!("{}/video.m4s", server.base_url())
                        },
                        {
                            "id": 64,
                            "baseUrl": format!("{}/video-64.m4s", server.base_url()),
                            "base_url": format!("{}/video-64.m4s", server.base_url())
                        }
                    ],
                    "audio": [
                        {
                            "id": 30280,
                            "baseUrl": format!("{}/audio.m4s", server.base_url()),
                            "base_url": format!("{}/audio.m4s", server.base_url())
                        },
                        {
                            "id": 30216,
                            "baseUrl": format!("{}/audio-30216.m4s", server.base_url()),
                            "base_url": format!("{}/audio-30216.m4s", server.base_url())
                        }
                    ]
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
            "data": {
                "subtitle": {
                    "subtitles": [{
                        "lan": "en",
                        "lan_doc": "English",
                        "subtitle_url": format!("{}/subtitle.ass", server.base_url())
                    }]
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/video.m4s");
        then.status(200).body("video");
    });
    server.mock(|when, then| {
        when.method(GET).path("/video-64.m4s");
        then.status(200).body("video64");
    });
    server.mock(|when, then| {
        when.method(GET).path("/audio.m4s");
        then.status(200).body("audio");
    });
    server.mock(|when, then| {
        when.method(GET).path("/audio-30216.m4s");
        then.status(200).body("audio30216");
    });
    server.mock(|when, then| {
        when.method(GET).path("/subtitle.ass");
        then.status(200).body("[Script Info]");
    });
    server.mock(|when, then| {
        when.method(GET).path("/2.xml");
        then.status(200).body("<i/>");
    });

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("--comment-base")
        .arg(server.base_url())
        .arg("download")
        .arg("av170001")
        .arg("--output-dir")
        .arg(&output_dir)
        .arg("--video-quality")
        .arg("64")
        .arg("--audio-quality")
        .arg("30216")
        .arg("--no-mux")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(
        json["entries"][0]["files"].as_array().map(Vec::len),
        Some(4)
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "video")?)?,
        "video64"
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "audio")?)?,
        "audio30216"
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "subtitle")?)?,
        "[Script Info]"
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "danmaku")?)?,
        "<i/>"
    );
    Ok(())
}

#[test]
fn download_archive_cancel_reports_preflight_json() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    let archive_file = temp.path().join("archive.json");
    mock_minimal_download(&server);

    archive_download_command(&credential_file, &server, &output_dir, &archive_file, None)?
        .assert()
        .success();

    let output = archive_download_command(
        &credential_file,
        &server,
        &output_dir,
        &archive_file,
        Some("cancel"),
    )?
    .assert()
    .success()
    .get_output()
    .stdout
    .clone();
    let json: Value = serde_json::from_slice(&output)?;

    assert_eq!(json["status"], "canceled");
    assert_eq!(
        json["preflight"]["archived_records"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert!(
        json["preflight"]["output_conflict"]["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("Mock video"))
    );
    Ok(())
}

#[test]
fn download_archive_json_requires_explicit_duplicate_decision() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    let archive_file = temp.path().join("archive.json");
    mock_minimal_download(&server);

    archive_download_command(&credential_file, &server, &output_dir, &archive_file, None)?
        .assert()
        .success();

    let stderr =
        archive_download_command(&credential_file, &server, &output_dir, &archive_file, None)?
            .assert()
            .failure()
            .get_output()
            .stderr
            .clone();

    assert!(
        String::from_utf8_lossy(&stderr).contains("--on-duplicate replace, keep-both, or cancel")
    );
    Ok(())
}

#[test]
fn download_archive_keep_both_uses_suffixed_output_dir() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    let archive_file = temp.path().join("archive.json");
    mock_minimal_download(&server);

    archive_download_command(&credential_file, &server, &output_dir, &archive_file, None)?
        .assert()
        .success();

    let output = archive_download_command(
        &credential_file,
        &server,
        &output_dir,
        &archive_file,
        Some("keep-both"),
    )?
    .assert()
    .success()
    .get_output()
    .stdout
    .clone();
    let json: Value = serde_json::from_slice(&output)?;
    let archive: Value = serde_json::from_slice(&fs::read(&archive_file)?)?;

    assert!(
        json["output_dir"]
            .as_str()
            .is_some_and(|path| path.ends_with("Mock video (2)"))
    );
    assert!(output_dir.join("Mock video").exists());
    assert!(output_dir.join("Mock video (2)").exists());
    assert_eq!(archive["records"].as_array().map(Vec::len), Some(2));
    Ok(())
}

#[test]
fn download_archive_keep_both_allows_archive_inside_old_output_root() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    let archive_file = output_dir.join("Mock video").join("archive.json");
    mock_minimal_download(&server);
    fs::create_dir_all(output_dir.join("Mock video"))?;

    let output = archive_download_command(
        &credential_file,
        &server,
        &output_dir,
        &archive_file,
        Some("keep-both"),
    )?
    .assert()
    .success()
    .get_output()
    .stdout
    .clone();
    let json: Value = serde_json::from_slice(&output)?;
    let archive: Value = serde_json::from_slice(&fs::read(&archive_file)?)?;

    assert!(
        json["output_dir"]
            .as_str()
            .is_some_and(|path| path.ends_with("Mock video (2)"))
    );
    assert!(output_dir.join("Mock video").join("archive.json").exists());
    assert!(output_dir.join("Mock video (2)").exists());
    assert_eq!(archive["records"].as_array().map(Vec::len), Some(1));
    Ok(())
}

#[test]
fn download_archive_replace_overwrites_existing_file() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    let archive_file = temp.path().join("archive.json");
    mock_minimal_download(&server);

    let output =
        archive_download_command(&credential_file, &server, &output_dir, &archive_file, None)?
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
    let json: Value = serde_json::from_slice(&output)?;
    let video_path = downloaded_file_path(&json, "video")?.to_owned();
    fs::write(&video_path, "partial")?;

    let output = archive_download_command(
        &credential_file,
        &server,
        &output_dir,
        &archive_file,
        Some("replace"),
    )?
    .assert()
    .success()
    .get_output()
    .stdout
    .clone();
    let json: Value = serde_json::from_slice(&output)?;
    let archive: Value = serde_json::from_slice(&fs::read(&archive_file)?)?;

    assert_eq!(fs::read_to_string(&video_path)?, "video");
    assert_eq!(downloaded_file_path(&json, "video")?, video_path);
    assert_eq!(archive["records"].as_array().map(Vec::len), Some(1));
    Ok(())
}

#[test]
fn download_archive_rejects_archive_file_as_output_root() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    let archive_file = output_dir.join("Mock video");
    mock_minimal_download(&server);

    let stderr =
        archive_download_command(&credential_file, &server, &output_dir, &archive_file, None)?
            .assert()
            .failure()
            .get_output()
            .stderr
            .clone();

    assert!(String::from_utf8_lossy(&stderr).contains("must not overlap"));
    assert!(!archive_file.exists());
    Ok(())
}

#[test]
fn download_archive_rejects_archive_file_inside_output_root() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    let archive_file = output_dir.join("Mock video").join("archive.json");
    mock_minimal_download(&server);

    let stderr =
        archive_download_command(&credential_file, &server, &output_dir, &archive_file, None)?
            .assert()
            .failure()
            .get_output()
            .stderr
            .clone();

    assert!(String::from_utf8_lossy(&stderr).contains("must not overlap"));
    assert!(!archive_file.exists());
    Ok(())
}

#[test]
fn download_archive_rejects_archive_file_inside_keep_both_output_root() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    let archive_file = temp.path().join("archive.json");
    let keep_both_archive_file = output_dir.join("Mock video (2)").join("archive.json");
    mock_minimal_download(&server);

    archive_download_command(&credential_file, &server, &output_dir, &archive_file, None)?
        .assert()
        .success();

    let stderr = archive_download_command(
        &credential_file,
        &server,
        &output_dir,
        &keep_both_archive_file,
        Some("keep-both"),
    )?
    .assert()
    .failure()
    .get_output()
    .stderr
    .clone();

    assert!(String::from_utf8_lossy(&stderr).contains("must not overlap"));
    assert!(!keep_both_archive_file.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
#[allow(clippy::too_many_lines)]
fn download_json_default_mux_keeps_stdout_valid() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    let ffmpeg = write_fake_ffmpeg(
        temp.path(),
        "printf 'mux noise\\n'\nlast=\nfor arg do last=$arg; done\nprintf 'muxed' > \"$last\"\nexit 0",
    )?;
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
                        "baseUrl": format!("{}/video.m4s", server.base_url()),
                        "base_url": format!("{}/video.m4s", server.base_url()),
                        "size": 5
                    }],
                    "audio": [{
                        "id": 30280,
                        "baseUrl": format!("{}/audio.m4s", server.base_url()),
                        "base_url": format!("{}/audio.m4s", server.base_url()),
                        "size": 5
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
    server.mock(|when, then| {
        when.method(GET).path("/video.m4s");
        then.status(200).body("video");
    });
    server.mock(|when, then| {
        when.method(GET).path("/audio.m4s");
        then.status(200).body("audio");
    });

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("download")
        .arg("av170001")
        .arg("--output-dir")
        .arg(&output_dir)
        .arg("--ffmpeg")
        .arg(&ffmpeg)
        .arg("--no-danmaku")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(
        json["entries"][0]["files"].as_array().map(Vec::len),
        Some(2)
    );
    assert!(
        json["entries"][0]["mux"]["output_path"]
            .as_str()
            .is_some_and(|path| path.ends_with("Main.mp4"))
    );
    Ok(())
}

#[test]
fn auth_import_status_and_logout_use_local_store() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");

    bbdown_command()?
        .arg("--credential-file")
        .arg(&credential_file)
        .env("BBDOWN_COOKIE", "SESSDATA=secret")
        .args(["auth", "import-cookie"])
        .assert()
        .success();

    let output = bbdown_command()?
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

    bbdown_command()?
        .arg("--credential-file")
        .arg(&credential_file)
        .args(["auth", "logout"])
        .assert()
        .success();

    let output = bbdown_command()?
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

#[test]
fn auth_qr_login_web_and_tv_use_local_store() -> anyhow::Result<()> {
    let web_server = MockServer::start();
    let tv_server = MockServer::start();
    let unused_passport_server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    mock_web_qr_login(&web_server);
    mock_tv_qr_login(&tv_server);

    let output = run_web_qr_login(&credential_file, &web_server)?;
    let events = json_lines(&output)?;
    assert_eq!(events[0]["event"], "ticket");
    assert_eq!(
        events[0]["url"],
        "https://passport.example/scan?qrcode_key=WEBKEY"
    );
    assert_eq!(events[1]["event"], "saved");
    assert_eq!(events[1]["saved"]["has_cookie"], true);
    assert_eq!(events[1]["saved"]["has_access_key"], false);
    assert!(!String::from_utf8_lossy(&output).contains("sess"));

    let output = run_tv_qr_login(&credential_file, &unused_passport_server, &tv_server)?;
    let events = json_lines(&output)?;
    assert_eq!(events[0]["event"], "ticket");
    assert_eq!(events[0]["url"], "https://tv.example/scan");
    assert_eq!(events[1]["event"], "saved");
    assert_eq!(events[1]["saved"]["has_cookie"], true);
    assert_eq!(events[1]["saved"]["has_access_key"], false);
    assert_eq!(events[1]["saved"]["has_tv_access_key"], true);
    assert!(!String::from_utf8_lossy(&output).contains("ACCESS"));
    let saved: Value = serde_json::from_slice(&fs::read(&credential_file)?)?;
    assert_eq!(saved["access_key"], Value::Null);
    assert_eq!(saved["tv_access_key"], "ACCESS");
    Ok(())
}

#[test]
fn auth_qr_login_failures_do_not_save_credentials() -> anyhow::Result<()> {
    let expired_server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let expired_credential_file = temp.path().join("expired-credentials.json");
    mock_expired_web_qr_login(&expired_server);

    let expired_output = bbdown_command()?
        .arg("--credential-file")
        .arg(&expired_credential_file)
        .arg("--passport-base")
        .arg(expired_server.base_url())
        .args([
            "auth",
            "login-web",
            "--timeout-seconds",
            "2",
            "--poll-interval-seconds",
            "1",
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(String::from_utf8_lossy(&expired_output).contains("QR code expired"));
    assert!(!expired_credential_file.exists());

    let tv_server = MockServer::start();
    let timeout_credential_file = temp.path().join("timeout-credentials.json");
    let (slow_poll_base, shutdown, handle) = slow_poll_server()?;
    mock_tv_qr_create(&tv_server);

    let started = Instant::now();
    let timeout_output = bbdown_command()?
        .arg("--credential-file")
        .arg(&timeout_credential_file)
        .arg("--request-timeout-seconds")
        .arg("5")
        .arg("--tv-passport-base")
        .arg(tv_server.base_url())
        .arg("--tv-passport-poll-base")
        .arg(slow_poll_base)
        .args([
            "auth",
            "login-tv",
            "--timeout-seconds",
            "1",
            "--poll-interval-seconds",
            "1",
            "--json",
        ])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let elapsed = started.elapsed();
    let _ = shutdown.send(());
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("slow poll server thread panicked"))?;

    assert!(String::from_utf8_lossy(&timeout_output).contains("QR login timed out"));
    assert!(
        elapsed < Duration::from_secs(4),
        "QR timeout should bound a hung poll request, elapsed: {elapsed:?}"
    );
    assert!(!timeout_credential_file.exists());
    Ok(())
}

fn mock_web_qr_login(web_server: &MockServer) {
    web_server.mock(|when, then| {
        when.method(GET)
            .path("/x/passport-login/web/qrcode/generate")
            .query_param("source", "main-fe-header")
            .header_missing("cookie");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "url": "https://passport.example/scan?qrcode_key=WEBKEY",
                "qrcode_key": "WEBKEY"
            }
        }));
    });
    web_server.mock(|when, then| {
        when.method(GET)
            .path("/x/passport-login/web/qrcode/poll")
            .query_param("qrcode_key", "WEBKEY")
            .query_param("source", "main-fe-header")
            .header_missing("cookie");
        then.status(200)
            .header("Set-Cookie", "SESSDATA=sess; Path=/; Domain=.bilibili.com")
            .json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "code": 0,
                    "url": "https://passport.biligame.com/crossDomain?source=main_web&go_url=https%3A%2F%2Fpassport.bilibili.com"
                }
            }));
    });
}

fn mock_expired_web_qr_login(web_server: &MockServer) {
    web_server.mock(|when, then| {
        when.method(GET)
            .path("/x/passport-login/web/qrcode/generate")
            .query_param("source", "main-fe-header")
            .header_missing("cookie");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "url": "https://passport.example/scan?qrcode_key=EXPIRED",
                "qrcode_key": "EXPIRED"
            }
        }));
    });
    web_server.mock(|when, then| {
        when.method(GET)
            .path("/x/passport-login/web/qrcode/poll")
            .query_param("qrcode_key", "EXPIRED")
            .query_param("source", "main-fe-header")
            .header_missing("cookie");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "code": 86038,
                "message": "expired"
            }
        }));
    });
}

fn run_web_qr_login(
    credential_file: &std::path::Path,
    web_server: &MockServer,
) -> anyhow::Result<Vec<u8>> {
    Ok(bbdown_command()?
        .arg("--credential-file")
        .arg(credential_file)
        .arg("--passport-base")
        .arg(web_server.base_url())
        .args([
            "auth",
            "login-web",
            "--timeout-seconds",
            "2",
            "--poll-interval-seconds",
            "1",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone())
}

fn mock_tv_qr_create(tv_server: &MockServer) {
    tv_server.mock(|when, then| {
        when.method(POST)
            .path("/x/passport-tv-login/qrcode/auth_code")
            .header_missing("cookie")
            .form_urlencoded_tuple("auth_code", "")
            .form_urlencoded_tuple("mobi_app", "android_tv_yst")
            .form_urlencoded_tuple_exists("sign");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "url": "https://tv.example/scan",
                "auth_code": "AUTH"
            }
        }));
    });
}

fn mock_tv_qr_login(tv_server: &MockServer) {
    mock_tv_qr_create(tv_server);
    tv_server.mock(|when, then| {
        when.method(POST)
            .path("/x/passport-tv-login/qrcode/poll")
            .header_missing("cookie")
            .form_urlencoded_tuple("auth_code", "AUTH")
            .form_urlencoded_tuple_exists("sign");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {"access_token": "ACCESS"}
        }));
    });
}

fn run_tv_qr_login(
    credential_file: &std::path::Path,
    unused_passport_server: &MockServer,
    tv_server: &MockServer,
) -> anyhow::Result<Vec<u8>> {
    Ok(bbdown_command()?
        .arg("--credential-file")
        .arg(credential_file)
        .arg("--passport-base")
        .arg(unused_passport_server.base_url())
        .arg("--tv-passport-base")
        .arg(tv_server.base_url())
        .arg("--tv-passport-poll-base")
        .arg(tv_server.base_url())
        .args([
            "auth",
            "login-tv",
            "--timeout-seconds",
            "2",
            "--poll-interval-seconds",
            "1",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone())
}

fn slow_poll_server() -> anyhow::Result<(String, mpsc::Sender<()>, thread::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let base_url = format!("http://{}", listener.local_addr()?);
    let (shutdown_sender, shutdown_receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        loop {
            if shutdown_receiver.try_recv().is_ok() {
                break;
            }
            match listener.accept() {
                Ok((_stream, _address)) => {
                    let _ = shutdown_receiver.recv_timeout(Duration::from_secs(10));
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    Ok((base_url, shutdown_sender, handle))
}

#[cfg(unix)]
fn write_fake_ffmpeg(dir: &Path, body: &str) -> anyhow::Result<std::path::PathBuf> {
    let path = dir.join("fake-ffmpeg");
    fs::write(&path, format!("#!/bin/sh\n{body}\n"))?;
    let mut permissions = fs::metadata(&path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions)?;
    Ok(path)
}

fn mock_minimal_download(server: &MockServer) {
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
                        "baseUrl": format!("{}/video.m4s", server.base_url()),
                        "base_url": format!("{}/video.m4s", server.base_url())
                    }],
                    "audio": [{
                        "id": 30280,
                        "baseUrl": format!("{}/audio.m4s", server.base_url()),
                        "base_url": format!("{}/audio.m4s", server.base_url())
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
    server.mock(|when, then| {
        when.method(GET).path("/video.m4s");
        then.status(200).body("video");
    });
    server.mock(|when, then| {
        when.method(GET).path("/audio.m4s");
        then.status(200).body("audio");
    });
}

fn archive_download_command(
    credential_file: &Path,
    server: &MockServer,
    output_dir: &Path,
    archive_file: &Path,
    decision: Option<&str>,
) -> anyhow::Result<Command> {
    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("download")
        .arg("av170001")
        .arg("--output-dir")
        .arg(output_dir)
        .arg("--archive-file")
        .arg(archive_file)
        .arg("--no-mux")
        .arg("--no-subtitles")
        .arg("--no-danmaku")
        .arg("--json");
    if let Some(decision) = decision {
        command.arg("--on-duplicate").arg(decision);
    }
    Ok(command)
}

fn downloaded_file_path<'a>(json: &'a Value, kind: &str) -> anyhow::Result<&'a str> {
    json["entries"][0]["files"]
        .as_array()
        .and_then(|files| {
            files.iter().find_map(|file| {
                (file["kind"].as_str() == Some(kind))
                    .then(|| file["path"].as_str())
                    .flatten()
            })
        })
        .ok_or_else(|| anyhow::anyhow!("missing downloaded {kind} path"))
}

fn json_lines(output: &[u8]) -> anyhow::Result<Vec<Value>> {
    String::from_utf8(output.to_vec())?
        .lines()
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

fn bbdown_command() -> anyhow::Result<Command> {
    let mut command = Command::cargo_bin("bbdown")?;
    for name in CLI_OVERRIDE_ENV_VARS {
        command.env_remove(name);
    }
    Ok(command)
}
