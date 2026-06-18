use assert_cmd::Command;
use bbdown_core::{CredentialStore, Credentials};
use httpmock::MockServer;
use httpmock::prelude::*;
use prost::Message as _;
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
    "BBDOWN_TV_API_BASE",
    "BBDOWN_APP_GRPC_BASE",
    "BBDOWN_APP_PGC_GRPC_BASE",
    "BBDOWN_TV_PASSPORT_BASE",
    "BBDOWN_TV_PASSPORT_POLL_BASE",
    "BBDOWN_PLAYURL_MODE",
    "BBDOWN_RESTRICTED_AREA",
    "BBDOWN_RESTRICTED_AREA_PROXY",
    "BBDOWN_RESTRICTED_API_PROXY",
    "BBDOWN_CREDENTIAL_FILE",
    "BBDOWN_CREDENTIAL_PROFILE",
    "BBDOWN_REQUEST_TIMEOUT_SECONDS",
    "BBDOWN_COOKIE",
    "BBDOWN_ACCESS_KEY",
];

#[test]
fn cli_version_reports_package_version() -> anyhow::Result<()> {
    let mut command = bbdown_command()?;
    let output = command
        .arg("--version")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        String::from_utf8(output)?,
        format!("bbdown {}\n", env!("CARGO_PKG_VERSION"))
    );
    Ok(())
}

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
fn info_json_applies_video_index_range_selection() -> anyhow::Result<()> {
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
                "title": "Mock multi page video",
                "pages": [
                    {"page": 1, "cid": 11, "part": "P1"},
                    {"page": 2, "cid": 22, "part": "P2"},
                    {"page": 3, "cid": 33, "part": "P3"}
                ]
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

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("info")
        .arg("av170001")
        .arg("--select")
        .arg("2-3,1")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(json["video"]["pages"].as_array().map(Vec::len), Some(3));
    assert_eq!(json["video"]["pages"][0]["title"], "P2");
    assert_eq!(json["video"]["pages"][1]["title"], "P3");
    assert_eq!(json["video"]["pages"][2]["title"], "P1");
    Ok(())
}

#[test]
fn info_json_resolves_mock_favorite_collection() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    mock_favorite_collection(&server);

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("info")
        .arg("fav456")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(json["collection"]["collection"]["kind"], "favorite");
    assert_eq!(json["collection"]["collection"]["title"], "Favorite");
    assert_eq!(
        json["collection"]["selected_items"][0]["title"],
        "Saved video"
    );
    Ok(())
}

#[test]
fn info_json_resolves_mock_history_collection() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    mock_history_collection(&server);

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("info")
        .arg("history")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(json["collection"]["collection"]["kind"], "history");
    assert_eq!(json["collection"]["collection"]["title"], "History");
    assert_eq!(
        json["collection"]["selected_items"][0]["title"],
        "History video"
    );
    Ok(())
}

#[test]
fn info_json_resolves_mock_watch_later_collection() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    CredentialStore::new(credential_file.clone()).save(&Credentials {
        cookie: Some("SESSDATA=WEB_COOKIE".to_owned()),
        access_key: None,
        tv_access_key: None,
    })?;
    mock_watch_later_collection(&server);

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("info")
        .arg("watch-later")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(json["collection"]["collection"]["kind"], "watch_later");
    assert_eq!(json["collection"]["collection"]["title"], "Watch later");
    assert_eq!(
        json["collection"]["selected_items"][0]["title"],
        "Watch later video"
    );
    Ok(())
}

#[test]
fn info_json_resolves_mock_following_collection() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    CredentialStore::new(credential_file.clone()).save(&Credentials {
        cookie: Some("SESSDATA=WEB_COOKIE".to_owned()),
        access_key: None,
        tv_access_key: None,
    })?;
    mock_following_collection(&server);

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("info")
        .arg("following")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(json["collection"]["collection"]["kind"], "following");
    assert_eq!(
        json["collection"]["collection"]["title"],
        "Following videos"
    );
    assert_eq!(
        json["collection"]["selected_items"][0]["title"],
        "Following video"
    );
    Ok(())
}

#[test]
fn info_json_resolves_mock_recommendation_collection() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    mock_recommendation_collection(&server);

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("info")
        .arg("recommendations")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(json["collection"]["collection"]["kind"], "recommendation");
    assert_eq!(json["collection"]["collection"]["title"], "Recommendations");
    assert_eq!(
        json["collection"]["selected_items"][0]["title"],
        "Recommended video"
    );
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
                "pic": format!("{}/cover.jpg", server.base_url()),
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

#[test]
fn playback_json_resolves_media_request_specs() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    mock_playback_metadata(&server);
    mock_playback_streams(&server);
    let subtitle_mock = mock_empty_player_v2(&server, "170001", "2");

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("playback")
        .arg("av170001")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(json["title"], "Mock video");
    assert_eq!(json["entries"][0]["qualities"][0]["id"], 80);
    assert_eq!(json["entries"][0]["duration_seconds"], 90);
    assert_eq!(
        json["entries"][0]["variants"].as_array().map(Vec::len),
        Some(3)
    );
    assert_playback_abr_metadata(&json)?;
    let variant = &json["entries"][0]["variants"][0];
    assert_eq!(variant["kind"], "dash");
    assert_eq!(variant["bandwidth"], 1_328_000);
    assert_eq!(
        variant["codecs"],
        serde_json::json!(["avc1.640028", "mp4a.40.2"])
    );
    assert_eq!(
        variant["mime_types"],
        serde_json::json!(["video/mp4", "audio/mp4"])
    );
    assert_eq!(variant["width"], 1920);
    assert_eq!(variant["height"], 1080);
    assert_playback_selection_hints(&json);
    assert_eq!(
        variant["video"]["url"],
        "https://video.example/80.m4s?token=secret"
    );
    assert_eq!(
        variant["video"]["backup_urls"],
        serde_json::json!(["https://backup.example/80.m4s"])
    );
    assert_eq!(variant["video"]["headers"][0]["name"], "referer");
    assert_eq!(
        variant["video"]["headers"][0]["value"],
        "https://www.bilibili.com/"
    );
    assert_eq!(variant["video"]["headers"][1]["name"], "user-agent");
    assert_eq!(
        variant["video"]["cache_key"]["content_id"],
        "BV1xx411c7mD-cid2"
    );
    assert_eq!(variant["video"]["cache_key"]["media_kind"], "video");
    assert_eq!(variant["video"]["cache_key"]["stream_id"], 80);
    assert_eq!(
        variant["video"]["cache_key"]["source_hash"]
            .as_str()
            .map(str::len),
        Some(32)
    );
    assert_eq!(
        variant["audio"]["url"],
        "https://audio.example/30280.m4s?token=secret"
    );
    assert_eq!(variant["audio"]["cache_key"]["media_kind"], "audio");

    let mut human = bbdown_command()?;
    human
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("playback")
        .arg("av170001");
    let text = String::from_utf8(human.assert().success().get_output().stdout.clone())?;
    assert!(text.contains("variants: 3"));
    assert!(text.contains("kind=dash"));
    assert!(text.contains("abr=1/1 switchable=false"));
    assert!(text.contains("format=h264+aac"));
    assert!(text.contains("avc1.640028+mp4a.40.2"));
    assert!(text.contains("avplayer=preferred"));
    subtitle_mock.assert_calls(0);
    Ok(())
}

#[test]
fn playback_json_uses_tv_playurl_mode() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    CredentialStore::new(credential_file.clone())
        .save(&Credentials::default().with_tv_access_key("TV_ACCESS"))?;
    mock_playback_metadata(&server);
    let tv_playurl = server.mock(|when, then| {
        when.method(GET)
            .path("/x/tv/playurl")
            .query_param("access_key", "TV_ACCESS")
            .query_param("appkey", "4409e2ce8ffd12b8")
            .query_param("cid", "2")
            .query_param("mobi_app", "android_tv_yst")
            .query_param("object_id", "170001")
            .query_param("platform", "android")
            .query_param("playurl_type", "1")
            .query_param_exists("ts")
            .query_param_exists("sign");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "dash": {
                    "duration": 3,
                    "video": [{
                        "id": 80,
                        "base_url": "https://tv.example/video.m4s",
                        "codecs": "avc1.640028",
                        "bandwidth": 1_000_000,
                        "mime_type": "video/mp4"
                    }],
                    "audio": []
                }
            }
        }));
    });

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("--tv-api-base")
        .arg(server.base_url())
        .arg("--playurl-mode")
        .arg("tv")
        .arg("playback")
        .arg("av170001")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;

    tv_playurl.assert();
    assert_eq!(json["entries"][0]["source"], "normal_tv");
    assert_eq!(
        json["entries"][0]["variants"][0]["video"]["url"],
        "https://tv.example/video.m4s"
    );
    Ok(())
}

#[test]
fn playback_json_uses_app_playurl_mode() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    CredentialStore::new(credential_file.clone())
        .save(&Credentials::default().with_access_key("APP_ACCESS"))?;
    mock_playback_metadata(&server);
    let app_response = app_play_view_response_frame("https://app.example/video.m4s")?;
    let app_playurl = server.mock(|when, then| {
        when.method(POST)
            .path("/bilibili.app.playurl.v1.PlayURL/PlayView")
            .header("content-type", "application/grpc")
            .header("authorization", "identify_v1 APP_ACCESS")
            .header_exists("x-bili-metadata-bin")
            .header_missing("cookie");
        then.status(200).body(app_response.clone());
    });

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("--app-grpc-base")
        .arg(server.base_url())
        .arg("--playurl-mode")
        .arg("app")
        .arg("playback")
        .arg("av170001")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;

    app_playurl.assert();
    assert_eq!(json["entries"][0]["source"], "normal_app");
    assert_eq!(
        json["entries"][0]["variants"][0]["video"]["url"],
        "https://app.example/video.m4s"
    );
    assert_eq!(
        json["entries"][0]["variants"][0]["selection_hints"]["avplayer"]["video_codec_family"],
        "hevc"
    );
    assert_eq!(json["entries"][0]["variants"][0]["width"], 1920);
    assert_eq!(json["entries"][0]["variants"][0]["height"], 1080);
    assert_eq!(json["entries"][0]["variants"][0]["frame_rate"], "60");
    Ok(())
}

fn assert_playback_abr_metadata(json: &Value) -> anyhow::Result<()> {
    assert_eq!(
        json["entries"][0]["cache_key"]["content_id"],
        "BV1xx411c7mD-cid2"
    );
    let groups = json["entries"][0]["abr"]["groups"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("ABR groups should be an array"))?;
    let variant = &json["entries"][0]["variants"][0];
    assert_eq!(groups.len(), 3);
    let group_id = variant["abr"]["group_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("variant ABR group id should be a string"))?;
    let abr_group = groups
        .iter()
        .find(|group| group["id"].as_str() == Some(group_id))
        .ok_or_else(|| anyhow::anyhow!("variant ABR group should exist"))?;
    assert_eq!(abr_group["kind"], "dash_video");
    assert_eq!(abr_group["level_count"], 1);
    assert_eq!(abr_group["min_bandwidth"], 1_328_000);
    assert_eq!(abr_group["max_bandwidth"], 1_328_000);
    assert_eq!(abr_group["variant_ids"], serde_json::json!([variant["id"]]));
    assert_eq!(variant["cache_key"]["variant_kind"], "dash");
    assert_eq!(variant["cache_key"]["variant_id"], variant["id"]);
    assert_eq!(
        variant["cache_key"]["media_keys"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(variant["abr"]["group_id"], abr_group["id"]);
    assert_eq!(variant["abr"]["level_index"], 0);
    assert_eq!(variant["abr"]["level_count"], 1);
    assert_eq!(variant["abr"]["switchable"], false);
    Ok(())
}

fn app_play_view_response_frame(video_url: &str) -> anyhow::Result<Vec<u8>> {
    let reply = TestAppPlayViewReply {
        video_info: Some(TestAppVideoInfo {
            timelength: Some(3_000),
            stream_list: vec![TestAppStreamItem {
                stream_info: Some(TestAppStreamInfo {
                    quality: Some(80),
                    description: Some("1080P".to_owned()),
                }),
                dash_video: Some(TestAppDashVideo {
                    base_url: Some(video_url.to_owned()),
                    backup_url: Vec::new(),
                    bandwidth: Some(1_000_000),
                    codecid: Some(12),
                    size: Some(1000),
                    frame_rate: Some("60".to_owned()),
                    width: Some(1920),
                    height: Some(1080),
                }),
            }],
            dash_audio: vec![TestAppDashItem {
                id: Some(30280),
                base_url: Some("https://app.example/audio.m4s".to_owned()),
                backup_url: Vec::new(),
                bandwidth: Some(128_000),
                size: Some(300),
            }],
        }),
    };
    let payload = reply.encode_to_vec();
    let len = u32::try_from(payload.len())?;
    let mut frame = Vec::with_capacity(payload.len() + 5);
    frame.push(0);
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

#[derive(Clone, PartialEq, prost::Message)]
struct TestAppPlayViewReply {
    #[prost(message, optional, tag = "1")]
    video_info: Option<TestAppVideoInfo>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct TestAppVideoInfo {
    #[prost(uint64, optional, tag = "3")]
    timelength: Option<u64>,
    #[prost(message, repeated, tag = "5")]
    stream_list: Vec<TestAppStreamItem>,
    #[prost(message, repeated, tag = "6")]
    dash_audio: Vec<TestAppDashItem>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct TestAppStreamItem {
    #[prost(message, optional, tag = "1")]
    stream_info: Option<TestAppStreamInfo>,
    #[prost(message, optional, tag = "2")]
    dash_video: Option<TestAppDashVideo>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct TestAppStreamInfo {
    #[prost(uint32, optional, tag = "1")]
    quality: Option<u32>,
    #[prost(string, optional, tag = "3")]
    description: Option<String>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct TestAppDashVideo {
    #[prost(string, optional, tag = "1")]
    base_url: Option<String>,
    #[prost(string, repeated, tag = "2")]
    backup_url: Vec<String>,
    #[prost(uint32, optional, tag = "3")]
    bandwidth: Option<u32>,
    #[prost(uint32, optional, tag = "4")]
    codecid: Option<u32>,
    #[prost(uint64, optional, tag = "6")]
    size: Option<u64>,
    #[prost(string, optional, tag = "9")]
    frame_rate: Option<String>,
    #[prost(int32, optional, tag = "10")]
    width: Option<i32>,
    #[prost(int32, optional, tag = "11")]
    height: Option<i32>,
}

#[derive(Clone, PartialEq, prost::Message)]
struct TestAppDashItem {
    #[prost(uint32, optional, tag = "1")]
    id: Option<u32>,
    #[prost(string, optional, tag = "2")]
    base_url: Option<String>,
    #[prost(string, repeated, tag = "3")]
    backup_url: Vec<String>,
    #[prost(uint32, optional, tag = "4")]
    bandwidth: Option<u32>,
    #[prost(uint64, optional, tag = "7")]
    size: Option<u64>,
}

fn assert_playback_selection_hints(json: &Value) {
    let variant = &json["entries"][0]["variants"][0];
    let hint = &variant["selection_hints"]["avplayer"];
    assert_eq!(hint["format_key"], "h264+aac");
    assert_eq!(hint["video_codec"], "avc1.640028");
    assert_eq!(hint["audio_codec"], "mp4a.40.2");
    assert_eq!(hint["playable"], true);
    assert_eq!(hint["preferred"], true);
    assert_eq!(hint["video_codec_family"], "h264");
    assert_eq!(hint["audio_codec_family"], "aac");
    assert_eq!(
        hint["reasons"],
        serde_json::json!(["dash_container", "h264_video", "aac_audio"])
    );
    let hevc_variant = &json["entries"][0]["variants"][1];
    let hevc_hint = &hevc_variant["selection_hints"]["avplayer"];
    assert_eq!(hevc_hint["format_key"], "hevc+aac");
    assert_eq!(hevc_hint["video_codec"], "hev1.1.6.L120.90");
    assert_eq!(hevc_hint["playable"], true);
    assert_eq!(hevc_hint["video_codec_family"], "hevc");
    assert_eq!(hevc_hint["reasons"][1], "hevc_video");

    let av1_variant = &json["entries"][0]["variants"][2];
    let av1_hint = &av1_variant["selection_hints"]["avplayer"];
    assert_eq!(av1_hint["format_key"], "av1+aac");
    assert_eq!(av1_hint["video_codec"], "av01.0.08M.08");
    assert_eq!(av1_hint["playable"], true);
    assert_eq!(av1_hint["video_codec_family"], "av1");
    assert_eq!(av1_hint["reasons"][1], "av1_video");
}

fn mock_playback_metadata(server: &MockServer) {
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
                "pic": format!("{}/cover.jpg", server.base_url()),
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
}

fn mock_playback_streams(server: &MockServer) {
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
                "support_formats": [
                    {"quality": 80, "new_description": "1080P 高码率"},
                    {"quality": 64, "display_desc": "720P"}
                ],
                "dash": {
                    "duration": 90,
                    "video": [
                        {
                            "id": 80,
                            "baseUrl": "https://video.example/80.m4s?token=secret",
                            "backupUrl": ["https://backup.example/80.m4s"],
                            "codecs": "avc1.640028",
                            "bandwidth": 1_200_000,
                            "width": 1920,
                            "height": 1080,
                            "frameRate": "60",
                            "mimeType": "video/mp4",
                            "size": 10000
                        },
                        {
                            "id": 64,
                            "baseUrl": "https://video.example/64.m4s?token=secret",
                            "codecs": "hev1.1.6.L120.90",
                            "bandwidth": 800_000,
                            "width": 1280,
                            "height": 720,
                            "frameRate": "30",
                            "mimeType": "video/mp4",
                            "size": 8000
                        },
                        {
                            "id": 120,
                            "baseUrl": "https://video.example/120.m4s?token=secret",
                            "codecs": "av01.0.08M.08",
                            "bandwidth": 600_000,
                            "width": 1280,
                            "height": 720,
                            "frameRate": "30",
                            "mimeType": "video/mp4",
                            "size": 6000
                        }
                    ],
                    "audio": [{
                        "id": 30280,
                        "baseUrl": "https://audio.example/30280.m4s?token=secret",
                        "backupUrl": ["https://backup.example/30280.m4s"],
                        "codecs": "mp4a.40.2",
                        "bandwidth": 128_000,
                        "mimeType": "audio/mp4",
                        "size": 2000
                    }]
                }
            }
        }));
    });
}

fn mock_empty_player_v2<'a>(
    server: &'a MockServer,
    aid: &'static str,
    cid: &'static str,
) -> httpmock::Mock<'a> {
    server.mock(move |when, then| {
        when.method(GET)
            .path("/x/player/v2")
            .query_param("aid", aid)
            .query_param("cid", cid);
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {"subtitle": {"subtitles": []}}
        }));
    })
}

#[test]
fn plan_json_applies_index_range_selection() -> anyhow::Result<()> {
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
                "title": "Mock multi page video",
                "pages": [
                    {"page": 1, "cid": 11, "part": "P1"},
                    {"page": 2, "cid": 22, "part": "P2"},
                    {"page": 3, "cid": 33, "part": "P3"}
                ]
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
    for (cid, label) in [(22, "p2"), (33, "p3"), (11, "p1")] {
        server.mock(move |when, then| {
            when.method(GET)
                .path("/x/player/playurl")
                .query_param("avid", "170001")
                .query_param("cid", cid.to_string())
                .query_param("try_look", "1");
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {
                    "accept_quality": [80],
                    "support_formats": [{"quality": 80, "new_description": "1080P"}],
                    "dash": {
                        "duration": 3,
                        "video": [{
                            "id": 80,
                            "baseUrl": format!("https://video.example/{label}.m4s"),
                            "base_url": format!("https://video.example/{label}.m4s")
                        }],
                        "audio": [{
                            "id": 30280,
                            "baseUrl": format!("https://audio.example/{label}.m4s"),
                            "base_url": format!("https://audio.example/{label}.m4s")
                        }]
                    }
                }
            }));
        });
        server.mock(move |when, then| {
            when.method(GET)
                .path("/x/player/v2")
                .query_param("aid", "170001")
                .query_param("cid", cid.to_string());
            then.status(200).json_body_obj(&serde_json::json!({
                "code": 0,
                "data": {"subtitle": {"subtitles": []}}
            }));
        });
    }

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("plan")
        .arg("av170001")
        .arg("--select")
        .arg("2-3,1")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(json["entries"].as_array().map(Vec::len), Some(3));
    assert_eq!(json["entries"][0]["title"], "P2");
    assert_eq!(json["entries"][1]["title"], "P3");
    assert_eq!(json["entries"][2]["title"], "P1");
    Ok(())
}

#[test]
fn plan_json_resolves_mock_favorite_collection_streams() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    mock_favorite_collection(&server);
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/player/playurl")
            .query_param("avid", "170001")
            .query_param("cid", "9988")
            .query_param("try_look", "1");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "dash": {
                    "duration": 3,
                    "video": [{
                        "id": 80,
                        "baseUrl": "https://video.example/favorite.m4s",
                        "base_url": "https://video.example/favorite.m4s"
                    }],
                    "audio": [{
                        "id": 30280,
                        "baseUrl": "https://audio.example/favorite.m4s",
                        "base_url": "https://audio.example/favorite.m4s"
                    }]
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/player/v2")
            .query_param("aid", "170001")
            .query_param("cid", "9988");
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
        .arg("fav456")
        .arg("--select")
        .arg("page:1")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(json["title"], "Favorite");
    assert_eq!(json["entries"][0]["title"], "Saved video");
    assert_eq!(json["entries"][0]["streams"]["videos"][0]["id"], 80);
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

fn mock_favorite_collection(server: &MockServer) {
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/v3/fav/resource/list")
            .query_param("media_id", "456")
            .query_param("pn", "1")
            .query_param("ps", "20")
            .query_param("type", "0");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "info": {
                    "media_count": 1,
                    "title": "Favorite",
                    "intro": "Favorite intro",
                    "cover": "https://example.invalid/favorite.jpg",
                    "ctime": 1_700_000_000,
                    "upper": {"mid": 1, "name": "Tester"}
                },
                "medias": [{
                    "id": 170_001,
                    "type": 2,
                    "bvid": "BV1xx411c7mD",
                    "title": "Saved video",
                    "intro": "Saved intro",
                    "cover": "https://example.invalid/saved.jpg",
                    "pubtime": 1_700_000_001,
                    "duration": 3,
                    "attr": 0,
                    "page": 1,
                    "upper": {"mid": 1, "name": "Tester"},
                    "ugc": {"first_cid": 9988}
                }]
            }
        }));
    });
}

fn mock_history_collection(server: &MockServer) {
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/web-interface/history/cursor")
            .query_param("max", "0")
            .query_param("view_at", "0")
            .query_param("business", "")
            .query_param("type", "archive")
            .query_param("ps", "20");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "cursor": {
                    "max": 170_001,
                    "view_at": 1_700_000_000_i64,
                    "business": "archive"
                },
                "list": [{
                    "aid": 170_001,
                    "bvid": "BV1xx411c7mD",
                    "title": "History video",
                    "pic": "https://example.invalid/history.jpg",
                    "duration": 3,
                    "author_mid": 1,
                    "author_name": "Tester",
                    "history": {
                        "oid": 170_001,
                        "cid": 9988,
                        "page": 1,
                        "business": "archive"
                    }
                }]
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/web-interface/history/cursor")
            .query_param("max", "170001")
            .query_param("view_at", "1700000000")
            .query_param("business", "archive")
            .query_param("type", "archive")
            .query_param("ps", "20");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "cursor": {
                    "max": 170_001,
                    "view_at": 1_700_000_000_i64,
                    "business": "archive"
                },
                "list": []
            }
        }));
    });
}

fn mock_watch_later_collection(server: &MockServer) {
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/v2/history/toview")
            .header("cookie", "SESSDATA=WEB_COOKIE");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "count": 1,
                "list": [{
                    "aid": 170_001,
                    "bvid": "BV1xx411c7mD",
                    "cid": 0,
                    "title": "Watch later video",
                    "pic": "https://example.invalid/watch-later.jpg",
                    "page": {
                        "cid": 9988,
                        "page": 1,
                        "part": "Watch later video",
                        "duration": 3
                    },
                    "owner": {"mid": 1, "name": "Tester"}
                }]
            }
        }));
    });
}

fn mock_following_collection(server: &MockServer) {
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/polymer/web-dynamic/v1/feed/all")
            .query_param("type", "video")
            .query_param("platform", "web")
            .header("cookie", "SESSDATA=WEB_COOKIE");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "has_more": false,
                "offset": "offset-1",
                "items": [{
                    "type": "DYNAMIC_TYPE_AV",
                    "visible": true,
                    "modules": {
                        "module_author": {
                            "mid": 1,
                            "name": "Tester",
                            "pub_ts": 1_700_000_001_i64
                        },
                        "module_dynamic": {
                            "major": {
                                "type": "MAJOR_TYPE_ARCHIVE",
                                "archive": {
                                    "aid": "170001",
                                    "bvid": "BV1xx411c7mD",
                                    "cover": "https://example.invalid/following.jpg"
                                }
                            }
                        }
                    }
                }]
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/web-interface/view")
            .query_param("aid", "170001");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "aid": 170_001,
                "bvid": "BV1xx411c7mD",
                "title": "Following video",
                "desc": "Following description",
                "owner": {"mid": 1, "name": "Tester"},
                "pages": [{"page": 1, "cid": 9988, "part": "Main", "duration": 3}]
            }
        }));
    });
}

fn mock_recommendation_collection(server: &MockServer) {
    server.mock(|when, then| {
        when.method(GET).path("/x/web-interface/nav");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "wbi_img": {
                    "img_url": "https://i0.hdslb.com/bfs/wbi/0123456789abcdef0123456789abcdef.png",
                    "sub_url": "https://i0.hdslb.com/bfs/wbi/fedcba9876543210fedcba9876543210.png"
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/x/web-interface/wbi/index/top/feed/rcmd")
            .query_param("ps", "20")
            .query_param("fresh_idx", "1")
            .query_param_exists("wts")
            .query_param_exists("w_rid");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "item": [{
                    "goto": "av",
                    "id": 170_001,
                    "bvid": "BV1xx411c7mD",
                    "cid": 9988,
                    "pic": "https://example.invalid/recommendation.jpg",
                    "title": "Recommended video",
                    "duration": 3,
                    "owner": {"mid": 1, "name": "Tester"}
                }, {
                    "goto": "live",
                    "id": 170_002,
                    "title": "Skipped live recommendation"
                }]
            }
        }));
    });
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
                "pic": format!("{}/cover.jpg", server.base_url()),
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
        when.method(GET).path("/cover.jpg");
        then.status(200).body("cover");
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
        Some(5)
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
        fs::read_to_string(downloaded_file_path(&json, "cover")?)?,
        "cover"
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "danmaku")?)?,
        "<i/>"
    );
    Ok(())
}

#[test]
fn download_json_applies_path_templates() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    mock_minimal_download(&server);

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
        .arg("--output-template")
        .arg("{title}-{entry_count:02}")
        .arg("--entry-template")
        .arg("{index:02}-{entry_title}-{aid}-{cid}")
        .arg("--no-cover")
        .arg("--no-subtitles")
        .arg("--no-danmaku")
        .arg("--no-mux")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    let output_path = Path::new(
        json["output_dir"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing output_dir"))?,
    );
    let entry_path = Path::new(
        json["entries"][0]["directory"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing entry directory"))?,
    );

    assert_eq!(
        output_path.file_name().and_then(std::ffi::OsStr::to_str),
        Some("Mock video-01")
    );
    assert_eq!(
        entry_path.file_name().and_then(std::ffi::OsStr::to_str),
        Some("01-Main-170001-2")
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "video")?)?,
        "video"
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "audio")?)?,
        "audio"
    );
    Ok(())
}

#[test]
fn download_no_cover_skips_cover_sidecar() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    mock_minimal_download_with_cover(&server);

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
        .arg("--no-cover")
        .arg("--no-subtitles")
        .arg("--no-danmaku")
        .arg("--no-mux")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;

    assert_eq!(
        json["entries"][0]["files"].as_array().map(Vec::len),
        Some(2)
    );
    assert!(downloaded_file_path(&json, "cover").is_err());
    Ok(())
}

#[test]
fn download_only_modes_write_selected_file_kind() -> anyhow::Result<()> {
    for (only, kind, expected_body) in [
        ("video", "video", "video"),
        ("audio", "audio", "audio"),
        ("cover", "cover", "cover"),
        ("subtitle", "subtitle", "[Script Info]"),
        ("danmaku", "danmaku", "<i/>"),
    ] {
        let server = MockServer::start();
        let temp = tempfile::tempdir()?;
        let credential_file = temp.path().join("credentials.json");
        let output_dir = temp.path().join("downloads");
        mock_minimal_download_with_sidecars(&server);

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
            .arg("--only")
            .arg(only)
            .arg("--no-mux")
            .arg("--json");
        let output = command.assert().success().get_output().stdout.clone();
        let json: Value = serde_json::from_slice(&output)?;

        assert_eq!(
            json["entries"][0]["files"].as_array().map(Vec::len),
            Some(1),
            "--only {only} should write one file"
        );
        assert_eq!(
            fs::read_to_string(downloaded_file_path(&json, kind)?)?,
            expected_body,
            "--only {only} should write {kind}"
        );
    }
    Ok(())
}

#[test]
fn download_only_danmaku_can_write_ass_format() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    mock_minimal_download_with_sidecars(&server);

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
        .arg("--only")
        .arg("danmaku")
        .arg("--danmaku-format")
        .arg("ass")
        .arg("--no-mux")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;

    assert_eq!(
        json["entries"][0]["files"].as_array().map(Vec::len),
        Some(1)
    );
    let ass_path = downloaded_file_path(&json, "danmaku_ass")?;
    assert!(ass_path.ends_with("danmaku.ass"));
    assert!(fs::read_to_string(ass_path)?.contains("[Script Info]"));
    assert!(downloaded_file_path(&json, "danmaku").is_err());
    Ok(())
}

#[test]
fn download_only_danmaku_can_write_xml_and_ass_formats() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    mock_minimal_download_with_sidecars(&server);

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
        .arg("--only")
        .arg("danmaku")
        .arg("--danmaku-format")
        .arg("xml,ass")
        .arg("--no-mux")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;

    assert_eq!(
        json["entries"][0]["files"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "danmaku")?)?,
        "<i/>"
    );
    assert!(fs::read_to_string(downloaded_file_path(&json, "danmaku_ass")?)?.contains("[Events]"));
    Ok(())
}

#[test]
fn download_upos_host_rewrites_media_url_candidates() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    mock_minimal_download_with_remote_media_host(&server);

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
        .arg("--only")
        .arg("video")
        .arg("--upos-host")
        .arg(server_authority(&server)?)
        .arg("--no-mux")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;

    assert_eq!(
        json["entries"][0]["files"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "video")?)?,
        "rewritten-video"
    );
    Ok(())
}

#[test]
fn download_upos_host_rejects_path_query_or_fragment() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("download")
        .arg("av170001")
        .arg("--upos-host")
        .arg("upos.example/path");

    command.assert().failure().stderr(predicates::str::contains(
        "--upos-host expects only a host or host:port",
    ));
    Ok(())
}

#[test]
fn download_only_rejects_conflicting_disable_flag() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("download")
        .arg("av170001")
        .arg("--only")
        .arg("cover")
        .arg("--no-cover");

    command.assert().failure().stderr(predicates::str::contains(
        "--only cover conflicts with --no-cover",
    ));
    Ok(())
}

#[test]
fn download_only_archive_does_not_duplicate_full_download() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let archive_file = temp.path().join("archive.json");
    let sidecar_output_dir = temp.path().join("sidecar-downloads");
    let full_output_dir = temp.path().join("full-downloads");
    mock_minimal_download_with_sidecars(&server);

    let mut sidecar_command = bbdown_command()?;
    sidecar_command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("--comment-base")
        .arg(server.base_url())
        .arg("download")
        .arg("av170001")
        .arg("--output-dir")
        .arg(&sidecar_output_dir)
        .arg("--archive-file")
        .arg(&archive_file)
        .arg("--only")
        .arg("cover")
        .arg("--json")
        .assert()
        .success();

    let mut full_command = bbdown_command()?;
    full_command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("--comment-base")
        .arg(server.base_url())
        .arg("download")
        .arg("av170001")
        .arg("--output-dir")
        .arg(&full_output_dir)
        .arg("--archive-file")
        .arg(&archive_file)
        .arg("--no-mux")
        .arg("--no-subtitles")
        .arg("--no-danmaku")
        .arg("--json");
    let output = full_command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;

    assert_eq!(
        json["entries"][0]["files"].as_array().map(Vec::len),
        Some(3)
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "video")?)?,
        "video"
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "audio")?)?,
        "audio"
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "cover")?)?,
        "cover"
    );
    Ok(())
}

#[test]
fn download_only_cover_skips_playurl_resolution() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    mock_minimal_cover_metadata(&server);

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
        .arg("--only")
        .arg("cover")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;

    assert_eq!(
        json["entries"][0]["files"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "cover")?)?,
        "cover"
    );
    Ok(())
}

#[test]
fn download_only_cover_with_archive_skips_playurl_resolution() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let archive_file = temp.path().join("archive.json");
    let output_dir = temp.path().join("downloads");
    mock_minimal_cover_metadata(&server);

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
        .arg("--archive-file")
        .arg(&archive_file)
        .arg("--only")
        .arg("cover")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;

    assert_eq!(
        json["entries"][0]["files"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        fs::read_to_string(downloaded_file_path(&json, "cover")?)?,
        "cover"
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

#[cfg(unix)]
#[test]
fn download_archive_symlink_updates_target_archive() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    let shared_dir = temp.path().join("shared");
    let archive_target = shared_dir.join("archive.json");
    let archive_link = temp.path().join("archive-link.json");
    fs::create_dir_all(&shared_dir)?;
    fs::write(&archive_target, "{\"records\":[]}")?;
    std::os::unix::fs::symlink(&archive_target, &archive_link)?;
    mock_minimal_download(&server);

    archive_download_command(&credential_file, &server, &output_dir, &archive_link, None)?
        .assert()
        .success();
    let archive: Value = serde_json::from_slice(&fs::read(&archive_target)?)?;

    assert!(
        fs::symlink_metadata(&archive_link)?
            .file_type()
            .is_symlink()
    );
    assert_eq!(archive["records"].as_array().map(Vec::len), Some(1));
    assert!(output_dir.join("Mock video").exists());
    assert!(
        !temp
            .path()
            .join("archive-link.json.bbdown-archive-backup")
            .exists()
    );
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
fn danmaku_update_archive_appends_xml_and_writes_ass() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_dir = temp.path().join("downloads");
    let output_root = output_dir.join("Mock video");
    let entry_dir = output_root.join("P001-BV1xx411c7mD-Main");
    let archive_file = temp.path().join("archive.json");
    let xml_path = entry_dir.join("danmaku.xml");
    fs::create_dir_all(&entry_dir)?;
    fs::write(&xml_path, r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#)?;
    fs::write(
        &archive_file,
        serde_json::to_vec_pretty(&serde_json::json!({
            "records": [{
                "content_key": "plan|aid=170001;cid=2",
                "title": "Mock video",
                "output_dir": output_root.clone(),
                "completed_at_unix": 1,
                "entries": [{
                    "content_key": "aid=170001;cid=2",
                    "index": 1,
                    "aid": 170_001,
                    "bvid": "BV1xx411c7mD",
                    "cid": 2,
                    "epid": null,
                    "title": "Main",
                    "directory": entry_dir.clone(),
                    "files": [xml_path.clone()],
                    "mux_output": null
                }]
            }]
        }))?,
    )?;
    mock_minimal_video_metadata(&server);
    let danmaku_mock = server.mock(|when, then| {
        when.method(GET).path("/2.xml");
        then.status(200)
            .body(r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#);
    });

    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("--comment-base")
        .arg(server.base_url())
        .arg("danmaku")
        .arg("update")
        .arg("av170001")
        .arg("--archive-file")
        .arg(&archive_file)
        .arg("--danmaku-format")
        .arg("xml,ass")
        .arg("--json");
    let output = command.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output)?;
    let archive: Value = serde_json::from_slice(&fs::read(&archive_file)?)?;

    danmaku_mock.assert_calls(1);
    assert_eq!(json["entries"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["entries"][0]["existing_comments"], 1);
    assert_eq!(json["entries"][0]["fetched_comments"], 2);
    assert_eq!(json["entries"][0]["appended_comments"], 1);
    let merged_xml = fs::read_to_string(&xml_path)?;
    assert_eq!(merged_xml.matches("<d ").count(), 2);
    assert!(entry_dir.join("danmaku.ass").exists());
    assert!(
        archive["records"][0]["content_key"]
            .as_str()
            .is_some_and(|key| key.starts_with("mode=all;danmaku=xml+ass;plan|"))
    );
    assert!(
        archive["records"][0]["entries"][0]["content_key"]
            .as_str()
            .is_some_and(|key| key.starts_with("mode=all;danmaku=xml+ass;aid=170001;cid=2"))
    );
    assert_eq!(
        archive["records"][0]["entries"][0]["files"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert!(
        archive["records"][0]["entries"][0]["files"]
            .as_array()
            .is_some_and(|files| files.iter().any(|path| path
                .as_str()
                .is_some_and(|path| path.ends_with("danmaku.ass"))))
    );
    Ok(())
}

#[test]
fn danmaku_update_rejects_archive_file_that_overlaps_xml_sidecar() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_root = temp.path().join("downloads").join("Mock video");
    let entry_dir = output_root.join("P001-BV1xx411c7mD-Main");
    let archive_file = entry_dir.join("danmaku.xml");
    fs::create_dir_all(&entry_dir)?;
    let archive_bytes =
        write_mock_danmaku_update_archive(&archive_file, &output_root, &entry_dir, &archive_file)?;
    mock_minimal_video_metadata(&server);
    let danmaku_mock = server.mock(|when, then| {
        when.method(GET).path("/2.xml");
        then.status(200)
            .body(r#"<i><d p="1,1,25,0,0,0,0,0">new</d></i>"#);
    });

    let stderr = danmaku_update_command(&credential_file, &server, &archive_file)?
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();

    assert!(
        String::from_utf8_lossy(&stderr).contains("must not overwrite updated danmaku sidecars")
    );
    assert_eq!(fs::read(&archive_file)?, archive_bytes);
    danmaku_mock.assert_calls(0);
    Ok(())
}

#[test]
fn danmaku_update_rejects_archive_file_that_overlaps_source_temp_file() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_root = temp.path().join("downloads").join("Mock video");
    let entry_dir = output_root.join("P001-BV1xx411c7mD-Main");
    let xml_path = entry_dir.join("danmaku.xml");
    let archive_file = entry_dir.join("danmaku.xml.bbdown-source");
    fs::create_dir_all(&entry_dir)?;
    fs::write(&xml_path, r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#)?;
    let archive_bytes =
        write_mock_danmaku_update_archive(&archive_file, &output_root, &entry_dir, &xml_path)?;
    mock_minimal_video_metadata(&server);
    let danmaku_mock = server.mock(|when, then| {
        when.method(GET).path("/2.xml");
        then.status(200)
            .body(r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#);
    });

    let stderr = danmaku_update_command(&credential_file, &server, &archive_file)?
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();

    assert!(
        String::from_utf8_lossy(&stderr).contains("must not overwrite updated danmaku sidecars")
    );
    assert_eq!(fs::read(&archive_file)?, archive_bytes);
    assert_eq!(
        fs::read_to_string(&xml_path)?,
        r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#
    );
    danmaku_mock.assert_calls(0);
    Ok(())
}

#[test]
fn danmaku_update_rejects_archive_file_that_overlaps_source_download_temp_file()
-> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_root = temp.path().join("downloads").join("Mock video");
    let entry_dir = output_root.join("P001-BV1xx411c7mD-Main");
    let xml_path = entry_dir.join("danmaku.xml");
    let archive_file = entry_dir.join("danmaku.xml.bbdown-source.bbdown-download");
    fs::create_dir_all(&entry_dir)?;
    fs::write(&xml_path, r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#)?;
    let archive_bytes =
        write_mock_danmaku_update_archive(&archive_file, &output_root, &entry_dir, &xml_path)?;
    mock_minimal_video_metadata(&server);
    let danmaku_mock = server.mock(|when, then| {
        when.method(GET).path("/2.xml");
        then.status(200)
            .body(r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#);
    });

    let stderr = danmaku_update_command(&credential_file, &server, &archive_file)?
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();

    assert!(
        String::from_utf8_lossy(&stderr).contains("must not overwrite updated danmaku sidecars")
    );
    assert_eq!(fs::read(&archive_file)?, archive_bytes);
    assert_eq!(
        fs::read_to_string(&xml_path)?,
        r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#
    );
    danmaku_mock.assert_calls(0);
    Ok(())
}

#[test]
fn danmaku_update_rejects_archive_file_that_overlaps_source_replace_temp_file() -> anyhow::Result<()>
{
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_root = temp.path().join("downloads").join("Mock video");
    let entry_dir = output_root.join("P001-BV1xx411c7mD-Main");
    let xml_path = entry_dir.join("danmaku.xml");
    let archive_file = entry_dir.join("danmaku.xml.bbdown-source.bbdown-replace");
    fs::create_dir_all(&entry_dir)?;
    fs::write(&xml_path, r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#)?;
    let archive_bytes =
        write_mock_danmaku_update_archive(&archive_file, &output_root, &entry_dir, &xml_path)?;
    mock_minimal_video_metadata(&server);
    let danmaku_mock = server.mock(|when, then| {
        when.method(GET).path("/2.xml");
        then.status(200)
            .body(r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#);
    });

    let stderr = danmaku_update_command(&credential_file, &server, &archive_file)?
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();

    assert!(
        String::from_utf8_lossy(&stderr).contains("must not overwrite updated danmaku sidecars")
    );
    assert_eq!(fs::read(&archive_file)?, archive_bytes);
    assert_eq!(
        fs::read_to_string(&xml_path)?,
        r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#
    );
    danmaku_mock.assert_calls(0);
    Ok(())
}

#[test]
fn danmaku_update_rejects_archive_file_that_overlaps_xml_generated_temp_file() -> anyhow::Result<()>
{
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_root = temp.path().join("downloads").join("Mock video");
    let entry_dir = output_root.join("P001-BV1xx411c7mD-Main");
    let xml_path = entry_dir.join("danmaku.xml");
    let archive_file = entry_dir.join("danmaku.xml.bbdown-generated");
    fs::create_dir_all(&entry_dir)?;
    fs::write(&xml_path, r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#)?;
    let archive_bytes =
        write_mock_danmaku_update_archive(&archive_file, &output_root, &entry_dir, &xml_path)?;
    mock_minimal_video_metadata(&server);
    let danmaku_mock = server.mock(|when, then| {
        when.method(GET).path("/2.xml");
        then.status(200)
            .body(r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#);
    });

    let stderr = danmaku_update_command(&credential_file, &server, &archive_file)?
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();

    assert!(
        String::from_utf8_lossy(&stderr).contains("must not overwrite updated danmaku sidecars")
    );
    assert_eq!(fs::read(&archive_file)?, archive_bytes);
    danmaku_mock.assert_calls(0);
    Ok(())
}

#[test]
fn danmaku_update_rejects_archive_file_that_overlaps_xml_replace_temp_file() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_root = temp.path().join("downloads").join("Mock video");
    let entry_dir = output_root.join("P001-BV1xx411c7mD-Main");
    let xml_path = entry_dir.join("danmaku.xml");
    let archive_file = entry_dir.join("danmaku.xml.bbdown-replace");
    fs::create_dir_all(&entry_dir)?;
    fs::write(&xml_path, r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#)?;
    let archive_bytes =
        write_mock_danmaku_update_archive(&archive_file, &output_root, &entry_dir, &xml_path)?;
    mock_minimal_video_metadata(&server);
    let danmaku_mock = server.mock(|when, then| {
        when.method(GET).path("/2.xml");
        then.status(200)
            .body(r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#);
    });

    let stderr = danmaku_update_command(&credential_file, &server, &archive_file)?
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();

    assert!(
        String::from_utf8_lossy(&stderr).contains("must not overwrite updated danmaku sidecars")
    );
    assert_eq!(fs::read(&archive_file)?, archive_bytes);
    danmaku_mock.assert_calls(0);
    Ok(())
}

#[test]
fn danmaku_update_rejects_archive_file_that_overlaps_ass_generated_temp_file() -> anyhow::Result<()>
{
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_root = temp.path().join("downloads").join("Mock video");
    let entry_dir = output_root.join("P001-BV1xx411c7mD-Main");
    let xml_path = entry_dir.join("danmaku.xml");
    let archive_file = entry_dir.join("danmaku.ass.bbdown-generated");
    fs::create_dir_all(&entry_dir)?;
    fs::write(&xml_path, r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#)?;
    let archive_bytes =
        write_mock_danmaku_update_archive(&archive_file, &output_root, &entry_dir, &xml_path)?;
    mock_minimal_video_metadata(&server);
    let danmaku_mock = server.mock(|when, then| {
        when.method(GET).path("/2.xml");
        then.status(200)
            .body(r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#);
    });

    let mut command = danmaku_update_command(&credential_file, &server, &archive_file)?;
    command.arg("--danmaku-format").arg("ass");
    let stderr = command.assert().failure().get_output().stderr.clone();

    assert!(
        String::from_utf8_lossy(&stderr).contains("must not overwrite updated danmaku sidecars")
    );
    assert_eq!(fs::read(&archive_file)?, archive_bytes);
    danmaku_mock.assert_calls(0);
    Ok(())
}

#[test]
fn danmaku_update_rejects_archive_file_that_overlaps_ass_replace_temp_file() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output_root = temp.path().join("downloads").join("Mock video");
    let entry_dir = output_root.join("P001-BV1xx411c7mD-Main");
    let xml_path = entry_dir.join("danmaku.xml");
    let archive_file = entry_dir.join("danmaku.ass.bbdown-replace");
    fs::create_dir_all(&entry_dir)?;
    fs::write(&xml_path, r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#)?;
    let archive_bytes =
        write_mock_danmaku_update_archive(&archive_file, &output_root, &entry_dir, &xml_path)?;
    mock_minimal_video_metadata(&server);
    let danmaku_mock = server.mock(|when, then| {
        when.method(GET).path("/2.xml");
        then.status(200)
            .body(r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#);
    });

    let mut command = danmaku_update_command(&credential_file, &server, &archive_file)?;
    command.arg("--danmaku-format").arg("ass");
    let stderr = command.assert().failure().get_output().stderr.clone();

    assert!(
        String::from_utf8_lossy(&stderr).contains("must not overwrite updated danmaku sidecars")
    );
    assert_eq!(fs::read(&archive_file)?, archive_bytes);
    danmaku_mock.assert_calls(0);
    Ok(())
}

fn danmaku_update_command(
    credential_file: &Path,
    server: &MockServer,
    archive_file: &Path,
) -> anyhow::Result<Command> {
    let mut command = bbdown_command()?;
    command
        .arg("--credential-file")
        .arg(credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("--comment-base")
        .arg(server.base_url())
        .arg("danmaku")
        .arg("update")
        .arg("av170001")
        .arg("--archive-file")
        .arg(archive_file)
        .arg("--json");
    Ok(command)
}

fn write_mock_danmaku_update_archive(
    archive_file: &Path,
    output_root: &Path,
    entry_dir: &Path,
    recorded_file: &Path,
) -> anyhow::Result<Vec<u8>> {
    let archive_bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "records": [{
            "content_key": "plan",
            "title": "Mock video",
            "output_dir": output_root.to_path_buf(),
            "completed_at_unix": 1,
            "entries": [{
                "content_key": "entry",
                "index": 1,
                "aid": 170_001,
                "bvid": "BV1xx411c7mD",
                "cid": 2,
                "epid": null,
                "title": "Main",
                "directory": entry_dir.to_path_buf(),
                "files": [recorded_file.to_path_buf()],
                "mux_output": null
            }]
        }]
    }))?;
    fs::write(archive_file, &archive_bytes)?;
    Ok(archive_bytes)
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
fn auth_profile_selection_isolates_named_profile() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");

    bbdown_command()?
        .arg("--credential-file")
        .arg(&credential_file)
        .env("BBDOWN_COOKIE", "SESSDATA=default")
        .args(["auth", "import-cookie"])
        .assert()
        .success();

    bbdown_command()?
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--credential-profile")
        .arg("intl")
        .env("BBDOWN_ACCESS_KEY", "INTL_ACCESS")
        .args(["auth", "import-access-key"])
        .assert()
        .success();

    let default_output = bbdown_command()?
        .arg("--credential-file")
        .arg(&credential_file)
        .args(["auth", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let default_status: Value = serde_json::from_slice(&default_output)?;
    assert_eq!(default_status["has_cookie"], true);
    assert_eq!(default_status["has_access_key"], false);

    let intl_output = bbdown_command()?
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--credential-profile")
        .arg("intl")
        .args(["auth", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let intl_status: Value = serde_json::from_slice(&intl_output)?;
    assert_eq!(intl_status["has_cookie"], false);
    assert_eq!(intl_status["has_access_key"], true);

    bbdown_command()?
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--credential-profile")
        .arg("intl")
        .args(["auth", "logout"])
        .assert()
        .success();

    assert_eq!(
        CredentialStore::new(credential_file.clone())
            .load()?
            .cookie
            .as_deref(),
        Some("SESSDATA=default")
    );
    assert!(
        CredentialStore::new(credential_file)
            .load_profile("intl")?
            .is_empty()
    );
    Ok(())
}

#[test]
fn info_json_uses_selected_credential_profile() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let store = CredentialStore::new(credential_file.clone());
    store.save(&Credentials {
        cookie: Some("SESSDATA=default".to_owned()),
        access_key: None,
        tv_access_key: None,
    })?;
    store.save_profile(
        "web",
        &Credentials {
            cookie: Some("SESSDATA=WEB_COOKIE".to_owned()),
            access_key: None,
            tv_access_key: None,
        },
    )?;
    mock_watch_later_collection(&server);

    let output = bbdown_command()?
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--credential-profile")
        .arg("web")
        .arg("--api-base")
        .arg(server.base_url())
        .arg("info")
        .arg("watch-later")
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output)?;
    assert_eq!(json["collection"]["collection"]["kind"], "watch_later");
    assert_eq!(
        json["collection"]["selected_items"][0]["title"],
        "Watch later video"
    );
    Ok(())
}

#[test]
fn auth_health_reports_redacted_credential_probe_statuses() -> anyhow::Result<()> {
    let server = MockServer::start();
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    CredentialStore::new(credential_file.clone()).save(
        &Credentials::default()
            .with_cookie("SESSDATA=COOKIE_SECRET")
            .with_access_key("ACCESS_SECRET")
            .with_tv_access_key("TV_SECRET"),
    )?;
    let cookie_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/x/web-interface/nav")
            .header("cookie", "SESSDATA=COOKIE_SECRET");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {
                "isLogin": true,
                "wbi_img": {
                    "img_url": "https://i0.hdslb.com/bfs/wbi/0123456789abcdef0123456789abcdef.png",
                    "sub_url": "https://i0.hdslb.com/bfs/wbi/fedcba9876543210fedcba9876543210.png"
                }
            }
        }));
    });
    let access_key_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/x/passport-login/oauth2/info")
            .query_param("access_key", "ACCESS_SECRET")
            .query_param("appkey", "7d089525d3611b1c")
            .query_param("mobi_app", "bstar_a")
            .query_param_exists("ts")
            .query_param_exists("sign")
            .header_missing("cookie");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": 0,
            "data": {"mid": 1}
        }));
    });
    let tv_access_key_mock = server.mock(|when, then| {
        when.method(GET)
            .path("/x/passport-login/oauth2/info")
            .query_param("access_key", "TV_SECRET")
            .query_param("appkey", "4409e2ce8ffd12b8")
            .query_param("mobi_app", "android_tv_yst")
            .query_param_exists("ts")
            .query_param_exists("sign")
            .header_missing("cookie");
        then.status(200).json_body_obj(&serde_json::json!({
            "code": -101,
            "message": "TV_SECRET expired"
        }));
    });

    let output = bbdown_command()?
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--api-base")
        .arg(server.base_url())
        .arg("--passport-base")
        .arg(server.base_url())
        .arg("--tv-passport-base")
        .arg(server.base_url())
        .arg("--tv-passport-poll-base")
        .arg(server.base_url())
        .args(["auth", "health", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output)?;

    assert_eq!(report["credentials"]["has_cookie"], true);
    assert_eq!(report["credentials"]["has_access_key"], true);
    assert_eq!(report["credentials"]["has_tv_access_key"], true);
    assert_eq!(report["probes"][0]["kind"], "cookie");
    assert_eq!(report["probes"][0]["scope"], "web_cookie");
    assert_eq!(report["probes"][0]["status"], "valid");
    assert_eq!(report["probes"][1]["kind"], "access_key");
    assert_eq!(report["probes"][1]["scope"], "intl_bstar");
    assert_eq!(report["probes"][1]["status"], "valid");
    assert_eq!(report["probes"][2]["kind"], "tv_access_key");
    assert_eq!(report["probes"][2]["scope"], "tv");
    assert_eq!(report["probes"][2]["status"], "rejected");
    assert_eq!(report["probes"][2]["api_code"], -101);
    let output_text = String::from_utf8(output)?;
    for secret in ["COOKIE_SECRET", "ACCESS_SECRET", "TV_SECRET"] {
        assert!(!output_text.contains(secret));
    }
    cookie_mock.assert_calls(1);
    access_key_mock.assert_calls(1);
    tv_access_key_mock.assert_calls(1);
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
    assert_eq!(
        events[0]["qr_payload"],
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
    assert_eq!(events[0]["qr_payload"], "https://tv.example/scan");
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
fn auth_login_access_key_message_saves_redacted_credentials() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output = bbdown_command()?
        .arg("--credential-file")
        .arg(&credential_file)
        .args([
            "auth",
            "login-access-key",
            "--json",
            "--stdin",
            "--message-origin",
            "https://www.biliplus.com/login",
        ])
        .write_stdin(
            r#"balh-login-credentials: {"access_key":"ACCESS_SECRET","refresh_token":"REFRESH_SECRET"}"#,
        )
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let events = json_lines(&output)?;

    assert_eq!(events[0]["event"], "ticket");
    assert_eq!(events[0]["kind"], "access_key");
    assert_eq!(events[0]["message_origin"], "https://www.biliplus.com");
    assert_eq!(events[0]["callback_origin"], "https://www.bilibili.com");
    assert_eq!(events[1]["event"], "saved");
    assert_eq!(events[1]["saved"]["has_cookie"], false);
    assert_eq!(events[1]["saved"]["has_access_key"], true);
    assert_eq!(events[1]["saved"]["has_tv_access_key"], false);
    let output_text = String::from_utf8(output)?;
    for secret in ["ACCESS_SECRET", "REFRESH_SECRET"] {
        assert!(!output_text.contains(secret));
    }
    let saved = CredentialStore::new(credential_file).load()?;
    assert_eq!(saved.access_key.as_deref(), Some("ACCESS_SECRET"));
    assert_eq!(saved.cookie, None);
    assert_eq!(saved.tv_access_key, None);
    Ok(())
}

#[test]
fn auth_login_access_key_callback_url_saves_selected_profile() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    CredentialStore::new(credential_file.clone()).save(&Credentials {
        cookie: Some("SESSDATA=default".to_owned()),
        access_key: None,
        tv_access_key: None,
    })?;
    let output = bbdown_command()?
        .arg("--credential-file")
        .arg(&credential_file)
        .arg("--credential-profile")
        .arg("intl")
        .args([
            "auth",
            "login-access-key",
            "--json",
            "--stdin",
            "--callback-origin",
            "https://m.bilibili.com/watch",
        ])
        .write_stdin(
            "https://www.bilibili.com/callback?access_token=PROFILE_SECRET&refresh_token=PROFILE_REFRESH",
        )
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let events = json_lines(&output)?;

    assert_eq!(events[0]["callback_origin"], "https://m.bilibili.com");
    assert_eq!(events[1]["saved"]["has_access_key"], true);
    let output_text = String::from_utf8(output)?;
    for secret in ["PROFILE_SECRET", "PROFILE_REFRESH"] {
        assert!(!output_text.contains(secret));
    }
    let store = CredentialStore::new(credential_file);
    assert_eq!(store.load()?.access_key, None);
    assert_eq!(store.load()?.cookie.as_deref(), Some("SESSDATA=default"));
    assert_eq!(
        store.load_profile("intl")?.access_key.as_deref(),
        Some("PROFILE_SECRET")
    );
    Ok(())
}

#[test]
fn auth_login_access_key_failures_do_not_save_credentials() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output = bbdown_command()?
        .arg("--credential-file")
        .arg(&credential_file)
        .args(["auth", "login-access-key", "--json", "--stdin"])
        .write_stdin(r#"balh-login-credentials: {"refresh_token":"REFRESH_SECRET"}"#)
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(output)?;

    assert!(stderr.contains("access_key"));
    assert!(!stderr.contains("REFRESH_SECRET"));
    assert!(!credential_file.exists());
    Ok(())
}

#[test]
fn auth_login_access_key_requires_explicit_input_source() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let credential_file = temp.path().join("credentials.json");
    let output = bbdown_command()?
        .arg("--credential-file")
        .arg(&credential_file)
        .args(["auth", "login-access-key", "--json"])
        .write_stdin(r#"balh-login-credentials: {"access_key":"PIPE_SECRET"}"#)
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr)?;
    let stdout = String::from_utf8(output.stdout)?;

    assert!(stderr.contains("provide access-key login data through --stdin"));
    assert!(!stderr.contains("PIPE_SECRET"));
    assert!(!stdout.contains("PIPE_SECRET"));
    assert!(!credential_file.exists());
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

fn mock_minimal_video_metadata(server: &MockServer) {
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
}

fn mock_minimal_download(server: &MockServer) {
    mock_minimal_video_metadata(server);
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

fn mock_minimal_download_with_remote_media_host(server: &MockServer) {
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
                        "baseUrl": "http://pcdn.example:12000/video.m4s",
                        "base_url": "http://pcdn.example:12000/video.m4s"
                    }],
                    "audio": [{
                        "id": 30280,
                        "baseUrl": "http://pcdn.example:12000/audio.m4s",
                        "base_url": "http://pcdn.example:12000/audio.m4s"
                    }]
                }
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/video.m4s");
        then.status(200).body("rewritten-video");
    });
}

fn mock_minimal_download_with_cover(server: &MockServer) {
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
                "pic": format!("{}/cover.jpg", server.base_url()),
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
    server.mock(|when, then| {
        when.method(GET).path("/cover.jpg");
        then.status(200).body("cover");
    });
}

fn mock_minimal_download_with_sidecars(server: &MockServer) {
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
                "pic": format!("{}/cover.jpg", server.base_url()),
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
        when.method(GET).path("/audio.m4s");
        then.status(200).body("audio");
    });
    server.mock(|when, then| {
        when.method(GET).path("/cover.jpg");
        then.status(200).body("cover");
    });
    server.mock(|when, then| {
        when.method(GET).path("/subtitle.ass");
        then.status(200).body("[Script Info]");
    });
    server.mock(|when, then| {
        when.method(GET).path("/2.xml");
        then.status(200).body("<i/>");
    });
}

fn mock_minimal_cover_metadata(server: &MockServer) {
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
                "pic": format!("{}/cover.jpg", server.base_url()),
                "pages": [{"page": 1, "cid": 2, "part": "Main"}]
            }
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/cover.jpg");
        then.status(200).body("cover");
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

fn server_authority(server: &MockServer) -> anyhow::Result<String> {
    let parsed = url::Url::parse(&server.base_url())?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("mock server base URL has no host"))?;
    Ok(parsed
        .port()
        .map_or_else(|| host.to_owned(), |port| format!("{host}:{port}")))
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
