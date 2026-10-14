use anyhow::{Context as _, bail};
use bbdown_core::{
    BiliClient, ClientConfig, Credentials, EndpointConfig, Input, ResolvedContent, Selection,
    VideoCollectionKind,
};
use httpmock::prelude::*;
use std::time::Duration;

// Synthetic auth fixture: joey-private-v3 / access-a, active access role.
const AUTH_VALUE: &str = "codex_synth_v1_access_a";
// Synthetic device-cookie fixtures: joey-private-v3 / id-a and id-b.
const SERVER_BUVID3: &str = "codex_synth_v1_id_a";
const SERVER_BUVID4: &str = "codex_synth_v1_id_b";

fn spi_response() -> serde_json::Value {
    serde_json::json!({
        "code": 0,
        "data": {"b_3": SERVER_BUVID3, "b_4": SERVER_BUVID4}
    })
}

fn nav_response() -> serde_json::Value {
    serde_json::json!({
        "code": 0,
        "data": {
            "wbi_img": {
                "img_url": "https://i0.hdslb.com/bfs/wbi/0123456789abcdef0123456789abcdef.png",
                "sub_url": "https://i0.hdslb.com/bfs/wbi/fedcba9876543210fedcba9876543210.png"
            }
        }
    })
}

fn feed_response() -> serde_json::Value {
    serde_json::json!({
        "code": 0,
        "data": {
            "has_more": false,
            "items": [{
                "type": "DYNAMIC_TYPE_AV",
                "visible": true,
                "modules": {
                    "module_dynamic": {
                        "major": {
                            "type": "MAJOR_TYPE_ARCHIVE",
                            "archive": {"aid": 170_001, "bvid": "BV1xx411c7mD"}
                        }
                    }
                }
            }]
        }
    })
}

fn view_response() -> serde_json::Value {
    serde_json::json!({
        "code": 0,
        "data": {
            "aid": 170_001,
            "bvid": "BV1xx411c7mD",
            "title": "Synthetic dynamic video",
            "pages": [
                {"page": 1, "cid": 9001, "part": "First", "duration": 3},
                {"page": 2, "cid": 9002, "part": "Second", "duration": 4}
            ]
        }
    })
}

#[tokio::test]
async fn legacy_space_dynamic_resolve_initializes_device_cookie_and_selects_video_page()
-> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        let server = MockServer::start();
        let auth_cookie = format!("SESSDATA={AUTH_VALUE}");
        let configured_cookie = format!("{auth_cookie}; buvid3=; buvid4=; extra=preserved");
        let spi_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/frontend/finger/spi")
                .is_true(|request| request.headers().get("cookie").is_none());
            then.status(200).json_body_obj(&spi_response());
        });
        let nav_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/nav")
                .header("cookie", configured_cookie.as_str());
            then.status(200).json_body_obj(&nav_response());
        });
        let feed_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/polymer/web-dynamic/v1/feed/space")
                .query_param("host_mid", "441167301")
                .query_param_exists("wts")
                .query_param_exists("w_rid")
                .header(
                    "cookie",
                    format!(
                        "{auth_cookie}; extra=preserved; buvid3={SERVER_BUVID3}; buvid4={SERVER_BUVID4}"
                    ),
                );
            then.status(200).json_body_obj(&feed_response());
        });
        let view_mock = server.mock(|when, then| {
            when.method(GET)
                .path("/x/web-interface/view")
                .query_param("aid", "170001");
            then.status(200).json_body_obj(&view_response());
        });

        let credentials = Credentials::default().with_cookie(configured_cookie.clone());
        let config = ClientConfig::default()
            .with_endpoints(EndpointConfig::default().with_api_base(server.base_url()))
            .with_credentials(credentials.clone())
            .with_request_timeout(Duration::from_secs(3));
        let client = BiliClient::new(config.clone());
        let resolved = client
            .resolve(Input::SpaceDynamic(441_167_301), Some(Selection::Page(2)))
            .await?;

        let ResolvedContent::Collection(resolution) = resolved else {
            bail!("expected a collection resolution");
        };
        assert_eq!(
            resolution.collection.kind,
            VideoCollectionKind::SpaceDynamic
        );
        assert_eq!(resolution.collection.items.len(), 2);
        assert_eq!(resolution.collection.items[0].cid, 9001);
        assert_eq!(resolution.collection.items[1].cid, 9002);
        assert_eq!(resolution.selected_items.len(), 1);
        assert_eq!(resolution.selected_items[0].cid, 9002);
        assert_eq!(
            credentials.cookie.as_deref(),
            Some(configured_cookie.as_str())
        );
        assert_eq!(
            config.credentials.cookie.as_deref(),
            Some(configured_cookie.as_str())
        );

        spi_mock.assert_calls(1);
        nav_mock.assert_calls(1);
        feed_mock.assert_calls(1);
        view_mock.assert_calls(1);
        Ok(())
    })
    .await
    .context("mocked SpaceDynamic resolution exceeded its test deadline")??;
    Ok(())
}
