use crate::models::{CodecFamily, FlvSegment, MediaStream, StreamQuality, StreamSet};
use crate::{Error, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use prost::{Enumeration, Message};
use reqwest::header::{
    AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, USER_AGENT,
};
use std::io::{Read, Write};

pub(crate) const NORMAL_PLAYURL_PATH: &str = "/bilibili.app.playurl.v1.PlayURL/PlayView";
pub(crate) const PGC_PLAYURL_PATH: &str = "/bilibili.pgc.gateway.player.v2.PlayURL/PlayView";

const APP_DALVIK_VERSION: &str = "2.1.0";
const APP_OS_VERSION: &str = "11";
const APP_BRAND: &str = "M2012K11AC";
const APP_MODEL: &str = "Build/RKQ1.200826.002";
const APP_VERSION: &str = "7.32.0";
const APP_BUILD: i32 = 7_320_200;
const APP_CHANNEL: &str = "xiaomi_cn_tv.danmaku.bili_zm20200902";
const APP_CRONET_VERSION: &str = "1.36.1";
const APP_MOBI_APP: &str = "android";
const APP_FAUKES_APPKEY: &str = "android64";
const APP_SESSION_ID: &str = "dedf8669";
const APP_PLATFORM: &str = "android";
const APP_ENV: &str = "prod";
const APP_ID: i32 = 1;
const APP_REGION: &str = "CN";
const APP_LANGUAGE: &str = "zh";
const APP_NETWORK_OID: &str = "46007";
pub(crate) const APP_PREVIEW_ONLY_RESTRICTION_MESSAGE: &str =
    "APP playurl returned preview-only streams";

pub(crate) fn play_view_request_body(content_id: u64, cid: u64, is_pgc: bool) -> Result<Vec<u8>> {
    let content_id = i64::try_from(content_id)
        .map_err(|_| Error::InvalidInput("APP playurl content id is too large".to_owned()))?;
    let cid = i64::try_from(cid)
        .map_err(|_| Error::InvalidInput("APP playurl cid is too large".to_owned()))?;
    let request = PlayViewReq {
        ep_id: Some(content_id),
        cid: Some(cid),
        qn: Some(127),
        fnver: None,
        fnval: Some(4048),
        download: Some(0),
        force_host: Some(2),
        fourk: Some(true),
        spmid: Some("main.ugc-video-detail.0.0".to_owned()),
        from_spmid: Some("main.my-history.0.0".to_owned()),
        teenagers_mode: None,
        prefer_codec_type: Some(AppCodeType::Hevc.into()),
        is_preview: None,
        room_id: None,
        is_need_view_info: is_pgc.then_some(true),
    };
    encode_grpc_frame(&request.encode_to_vec(), true)
}

pub(crate) fn play_view_headers(access_key: Option<&str>) -> Result<HeaderMap> {
    let access_key = access_key.map(str::trim).filter(|value| !value.is_empty());
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/grpc"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&app_user_agent())
            .map_err(|_| Error::InvalidInput("invalid APP playurl user-agent header".to_owned()))?,
    );
    insert_header(&mut headers, "te", "trailers")?;
    insert_header(
        &mut headers,
        "x-bili-fawkes-req-bin",
        &base64_protobuf(&FawkesReq {
            appkey: Some(APP_FAUKES_APPKEY.to_owned()),
            env: Some(APP_ENV.to_owned()),
            session_id: Some(APP_SESSION_ID.to_owned()),
        }),
    )?;
    insert_header(
        &mut headers,
        "x-bili-metadata-bin",
        &base64_protobuf(&Metadata {
            access_key: Some(access_key.unwrap_or_default().to_owned()),
            mobi_app: Some(APP_MOBI_APP.to_owned()),
            name: None,
            build: Some(APP_BUILD),
            channel: Some(APP_CHANNEL.to_owned()),
            buvid: Some(String::new()),
            platform: Some(APP_PLATFORM.to_owned()),
        }),
    )?;
    if let Some(access_key) = access_key {
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("identify_v1 {access_key}")).map_err(|_| {
                Error::InvalidInput("invalid APP playurl access key header".to_owned())
            })?,
        );
    }
    insert_header(
        &mut headers,
        "x-bili-device-bin",
        &base64_protobuf(&Device {
            app_id: Some(APP_ID),
            build: Some(APP_BUILD),
            buvid: Some(String::new()),
            mobi_app: Some(APP_MOBI_APP.to_owned()),
            platform: Some(APP_PLATFORM.to_owned()),
            name: None,
            channel: Some(APP_CHANNEL.to_owned()),
            brand: Some(APP_BRAND.to_owned()),
            model: Some(APP_MODEL.to_owned()),
            osver: Some(APP_OS_VERSION.to_owned()),
            fp_local: None,
            fp_remote: None,
            version_name: None,
            fp: None,
        }),
    )?;
    insert_header(
        &mut headers,
        "x-bili-network-bin",
        &base64_protobuf(&Network {
            r#type: Some(NetworkType::Wifi.into()),
            tf: None,
            oid: Some(APP_NETWORK_OID.to_owned()),
        }),
    )?;
    insert_header(&mut headers, "x-bili-restriction-bin", "")?;
    insert_header(
        &mut headers,
        "x-bili-locale-bin",
        &base64_protobuf(&Locale {
            c_locale: Some(LocaleIds {
                language: Some(APP_LANGUAGE.to_owned()),
                script: None,
                region: Some(APP_REGION.to_owned()),
            }),
            s_locale: None,
        }),
    )?;
    insert_header(&mut headers, "x-bili-exps-bin", "")?;
    insert_header(&mut headers, "grpc-encoding", "gzip")?;
    insert_header(&mut headers, "grpc-accept-encoding", "identity,gzip")?;
    insert_header(&mut headers, "grpc-timeout", "17996161u")?;
    Ok(headers)
}

pub(crate) fn decode_play_view_response(frame: &[u8]) -> Result<StreamSet> {
    let payload = decode_grpc_frame(frame)?;
    let reply = PlayViewReply::decode(payload.as_slice()).map_err(|error| {
        Error::InvalidInput(format!("invalid APP playurl protobuf response: {error}"))
    })?;
    reply.into_stream_set()
}

fn app_user_agent() -> String {
    format!(
        "Dalvik/{APP_DALVIK_VERSION} (Linux; U; Android {APP_OS_VERSION}; {APP_BRAND} {APP_MODEL}) {APP_VERSION} os/android model/{APP_BRAND} mobi_app/android build/{APP_BUILD} channel/{APP_CHANNEL} innerVer/{APP_BUILD} osVer/{APP_OS_VERSION} network/2 grpc-java-cronet/{APP_CRONET_VERSION}"
    )
}

fn insert_header(headers: &mut HeaderMap, name: &'static str, value: &str) -> Result<()> {
    headers.insert(
        HeaderName::from_static(name),
        HeaderValue::from_str(value)
            .map_err(|_| Error::InvalidInput(format!("invalid APP playurl header: {name}")))?,
    );
    Ok(())
}

fn base64_protobuf(message: &impl Message) -> String {
    STANDARD.encode(message.encode_to_vec())
}

fn encode_grpc_frame(payload: &[u8], compressed: bool) -> Result<Vec<u8>> {
    let payload = if compressed {
        gzip_compress(payload)?
    } else {
        payload.to_vec()
    };
    let len = u32::try_from(payload.len())
        .map_err(|_| Error::InvalidInput("APP playurl gRPC payload is too large".to_owned()))?;
    let mut frame = Vec::with_capacity(payload.len() + 5);
    frame.push(u8::from(compressed));
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn decode_grpc_frame(frame: &[u8]) -> Result<Vec<u8>> {
    if frame.len() < 5 {
        return Err(Error::MissingField("APP playurl gRPC frame"));
    }
    let compressed = frame[0];
    let mut len = [0_u8; 4];
    len.copy_from_slice(&frame[1..5]);
    let len = usize::try_from(u32::from_be_bytes(len))
        .map_err(|_| Error::InvalidInput("APP playurl gRPC frame length is invalid".to_owned()))?;
    let end = 5_usize
        .checked_add(len)
        .ok_or_else(|| Error::InvalidInput("APP playurl gRPC frame length overflow".to_owned()))?;
    let payload = frame
        .get(5..end)
        .ok_or(Error::MissingField("APP playurl gRPC payload"))?;
    match compressed {
        0 => Ok(payload.to_vec()),
        1 => gzip_decompress(payload),
        value => Err(Error::InvalidInput(format!(
            "unsupported APP playurl gRPC compression flag: {value}"
        ))),
    }
}

fn gzip_compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(data);
    let mut output = Vec::new();
    decoder.read_to_end(&mut output)?;
    Ok(output)
}

#[derive(Clone, PartialEq, Message)]
struct PlayViewReq {
    #[prost(int64, optional, tag = "1")]
    ep_id: Option<i64>,
    #[prost(int64, optional, tag = "2")]
    cid: Option<i64>,
    #[prost(int64, optional, tag = "3")]
    qn: Option<i64>,
    #[prost(int32, optional, tag = "4")]
    fnver: Option<i32>,
    #[prost(int32, optional, tag = "5")]
    fnval: Option<i32>,
    #[prost(uint32, optional, tag = "6")]
    download: Option<u32>,
    #[prost(int32, optional, tag = "7")]
    force_host: Option<i32>,
    #[prost(bool, optional, tag = "8")]
    fourk: Option<bool>,
    #[prost(string, optional, tag = "9")]
    spmid: Option<String>,
    #[prost(string, optional, tag = "10")]
    from_spmid: Option<String>,
    #[prost(int32, optional, tag = "11")]
    teenagers_mode: Option<i32>,
    #[prost(enumeration = "AppCodeType", optional, tag = "12")]
    prefer_codec_type: Option<i32>,
    #[prost(bool, optional, tag = "13")]
    is_preview: Option<bool>,
    #[prost(int64, optional, tag = "14")]
    room_id: Option<i64>,
    #[prost(bool, optional, tag = "15")]
    is_need_view_info: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Enumeration)]
#[repr(i32)]
enum AppCodeType {
    None = 0,
    H264 = 1,
    Hevc = 2,
    Av1 = 3,
}

#[derive(Clone, PartialEq, Message)]
struct VideoInfo {
    #[prost(uint32, optional, tag = "1")]
    quality: Option<u32>,
    #[prost(string, optional, tag = "2")]
    format: Option<String>,
    #[prost(uint64, optional, tag = "3")]
    timelength: Option<u64>,
    #[prost(uint32, optional, tag = "4")]
    video_codecid: Option<u32>,
    #[prost(message, repeated, tag = "5")]
    stream_list: Vec<StreamItem>,
    #[prost(message, repeated, tag = "6")]
    dash_audio: Vec<DashItem>,
    #[prost(message, optional, tag = "7")]
    dolby: Option<DolbyItem>,
    #[prost(message, optional, tag = "9")]
    flac: Option<DolbyItem>,
}

#[derive(Clone, PartialEq, Message)]
struct DolbyItem {
    #[prost(int32, optional, tag = "1")]
    r#type: Option<i32>,
    #[prost(message, optional, tag = "2")]
    audio: Option<DashItem>,
}

#[derive(Clone, PartialEq, Message)]
struct DashItem {
    #[prost(uint32, optional, tag = "1")]
    id: Option<u32>,
    #[prost(string, optional, tag = "2")]
    base_url: Option<String>,
    #[prost(string, repeated, tag = "3")]
    backup_url: Vec<String>,
    #[prost(uint32, optional, tag = "4")]
    bandwidth: Option<u32>,
    #[prost(uint32, optional, tag = "5")]
    codecid: Option<u32>,
    #[prost(string, optional, tag = "6")]
    md5: Option<String>,
    #[prost(uint64, optional, tag = "7")]
    size: Option<u64>,
}

#[derive(Clone, PartialEq, Message)]
struct StreamItem {
    #[prost(message, optional, tag = "1")]
    stream_info: Option<StreamInfo>,
    #[prost(message, optional, tag = "2")]
    dash_video: Option<DashVideo>,
    #[prost(message, optional, tag = "3")]
    segment_video: Option<SegmentVideo>,
}

#[derive(Clone, PartialEq, Message)]
struct StreamInfo {
    #[prost(uint32, optional, tag = "1")]
    quality: Option<u32>,
    #[prost(string, optional, tag = "2")]
    format: Option<String>,
    #[prost(string, optional, tag = "3")]
    description: Option<String>,
    #[prost(uint32, optional, tag = "4")]
    err_code: Option<u32>,
    #[prost(message, optional, tag = "5")]
    limit: Option<StreamLimit>,
    #[prost(bool, optional, tag = "6")]
    need_vip: Option<bool>,
    #[prost(bool, optional, tag = "7")]
    need_login: Option<bool>,
    #[prost(bool, optional, tag = "8")]
    intact: Option<bool>,
    #[prost(bool, optional, tag = "9")]
    no_rexcode: Option<bool>,
    #[prost(uint64, optional, tag = "10")]
    attribute: Option<u64>,
}

#[derive(Clone, PartialEq, Message)]
struct DashVideo {
    #[prost(string, optional, tag = "1")]
    base_url: Option<String>,
    #[prost(string, repeated, tag = "2")]
    backup_url: Vec<String>,
    #[prost(uint32, optional, tag = "3")]
    bandwidth: Option<u32>,
    #[prost(uint32, optional, tag = "4")]
    codecid: Option<u32>,
    #[prost(string, optional, tag = "5")]
    md5: Option<String>,
    #[prost(uint64, optional, tag = "6")]
    size: Option<u64>,
    #[prost(uint32, optional, tag = "7")]
    audio_id: Option<u32>,
    #[prost(bool, optional, tag = "8")]
    no_rexcode: Option<bool>,
    #[prost(string, optional, tag = "9")]
    frame_rate: Option<String>,
    #[prost(int32, optional, tag = "10")]
    width: Option<i32>,
    #[prost(int32, optional, tag = "11")]
    height: Option<i32>,
}

#[derive(Clone, PartialEq, Message)]
struct StreamLimit {
    #[prost(string, optional, tag = "1")]
    title: Option<String>,
    #[prost(string, optional, tag = "2")]
    uri: Option<String>,
    #[prost(string, optional, tag = "3")]
    msg: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
struct SegmentVideo {
    #[prost(message, repeated, tag = "1")]
    segment: Vec<ResponseUrl>,
}

#[derive(Clone, PartialEq, Message)]
struct ResponseUrl {
    #[prost(uint32, optional, tag = "1")]
    order: Option<u32>,
    #[prost(uint64, optional, tag = "2")]
    length: Option<u64>,
    #[prost(uint64, optional, tag = "3")]
    size: Option<u64>,
    #[prost(string, optional, tag = "4")]
    url: Option<String>,
    #[prost(string, repeated, tag = "5")]
    backup_url: Vec<String>,
    #[prost(string, optional, tag = "6")]
    md5: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
struct PlayViewReply {
    #[prost(message, optional, tag = "1")]
    video_info: Option<VideoInfo>,
    #[prost(message, optional, tag = "3")]
    business: Option<BusinessInfo>,
    #[prost(message, optional, tag = "5")]
    view_info: Option<ViewInfo>,
    #[prost(message, optional, tag = "9")]
    pgc_view_info: Option<ViewInfo>,
}

#[derive(Clone, PartialEq, Message)]
struct BusinessInfo {
    #[prost(bool, optional, tag = "1")]
    is_preview: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
struct ViewInfo {
    #[prost(message, optional, tag = "1")]
    dialog: Option<Dialog>,
    #[prost(message, optional, tag = "2")]
    toast: Option<Toast>,
}

#[derive(Clone, PartialEq, Message)]
struct Dialog {
    #[prost(int64, optional, tag = "1")]
    code: Option<i64>,
    #[prost(string, optional, tag = "2")]
    msg: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
struct Toast {
    #[prost(string, optional, tag = "1")]
    text: Option<String>,
    #[prost(message, optional, tag = "5")]
    toast_text: Option<TextInfo>,
}

#[derive(Clone, PartialEq, Message)]
struct TextInfo {
    #[prost(string, optional, tag = "1")]
    text: Option<String>,
}

impl PlayViewReply {
    fn into_stream_set(self) -> Result<StreamSet> {
        let access_limit_message = self.access_limit_message();
        if self.is_preview_only() {
            return Err(Error::AccessRestricted(
                access_limit_message
                    .unwrap_or_else(|| APP_PREVIEW_ONLY_RESTRICTION_MESSAGE.to_owned()),
            ));
        }
        let video_info = self.video_info.ok_or_else(|| {
            access_limit_message
                .clone()
                .map_or(Error::MissingField("videoInfo"), Error::AccessRestricted)
        })?;
        let fallback_quality = video_info.quality;
        let fallback_video_codecid = video_info.video_codecid;
        let timelength = video_info.timelength;
        let duration_seconds = video_info
            .timelength
            .and_then(|value| u32::try_from(value / 1000).ok());
        let mut qualities = Vec::new();
        let (videos, flv_segments) = collect_stream_list(
            video_info.stream_list,
            fallback_quality,
            fallback_video_codecid,
            timelength,
            &mut qualities,
        );
        let audios =
            collect_audio_streams(video_info.dash_audio, video_info.flac, video_info.dolby);
        if videos.is_empty()
            && flv_segments.is_empty()
            && let Some(message) = access_limit_message.clone()
        {
            return Err(Error::AccessRestricted(message));
        }
        if videos.is_empty() && audios.is_empty() && flv_segments.is_empty() {
            if let Some(message) = access_limit_message {
                return Err(Error::AccessRestricted(message));
            }
            return Err(Error::MissingField("APP playurl streams"));
        }
        let accept_quality = qualities
            .iter()
            .map(|quality| quality.id)
            .collect::<Vec<_>>();
        Ok(StreamSet {
            videos,
            audios,
            flv_segments,
            accept_quality,
            qualities,
            duration_seconds,
        })
    }

    fn access_limit_message(&self) -> Option<String> {
        self.view_info
            .as_ref()
            .and_then(ViewInfo::access_limit_message)
            .or_else(|| {
                self.pgc_view_info
                    .as_ref()
                    .and_then(ViewInfo::access_limit_message)
            })
            .or_else(|| {
                self.video_info.as_ref().and_then(|video_info| {
                    video_info
                        .stream_list
                        .iter()
                        .filter_map(|item| item.stream_info.as_ref())
                        .find_map(StreamInfo::access_limit_message)
                })
            })
    }

    fn is_preview_only(&self) -> bool {
        self.business
            .as_ref()
            .and_then(|business| business.is_preview)
            .unwrap_or(false)
    }
}

fn collect_stream_list(
    stream_list: Vec<StreamItem>,
    fallback_quality: Option<u32>,
    fallback_video_codecid: Option<u32>,
    timelength: Option<u64>,
    qualities: &mut Vec<StreamQuality>,
) -> (Vec<MediaStream>, Vec<FlvSegment>) {
    let mut videos = Vec::new();
    let mut selected_flv: Option<FlvCandidate> = None;
    for item in stream_list {
        let stream_info = item.stream_info;
        if let Some(dash_video) = item.dash_video
            && let Some(stream) = dash_video.into_media_stream(
                stream_info.as_ref(),
                fallback_quality,
                fallback_video_codecid,
                timelength,
            )
        {
            push_stream_quality(
                qualities,
                stream.id,
                stream_info.as_ref().and_then(|info| {
                    info.description
                        .as_ref()
                        .filter(|value| !value.is_empty())
                        .cloned()
                }),
            );
            videos.push(stream);
        }
        if let Some(segment_video) = item.segment_video {
            let segments = segment_video
                .segment
                .into_iter()
                .enumerate()
                .filter_map(|(index, segment)| segment.into_flv_segment(index))
                .collect::<Vec<_>>();
            if !segments.is_empty() {
                let candidate = FlvCandidate {
                    quality: stream_info
                        .as_ref()
                        .and_then(|info| info.quality)
                        .or(fallback_quality),
                    description: stream_info.as_ref().and_then(|info| {
                        info.description
                            .as_ref()
                            .filter(|value| !value.is_empty())
                            .cloned()
                    }),
                    segments,
                };
                if should_replace_flv_candidate(selected_flv.as_ref(), &candidate) {
                    selected_flv = Some(candidate);
                }
            }
        }
    }
    let mut flv_segments = Vec::new();
    if let Some(candidate) = selected_flv {
        if let Some(quality) = candidate.quality {
            push_stream_quality(qualities, quality, candidate.description);
        }
        flv_segments = candidate.segments;
    }
    (videos, flv_segments)
}

fn collect_audio_streams(
    dash_audio: Vec<DashItem>,
    flac: Option<DolbyItem>,
    dolby: Option<DolbyItem>,
) -> Vec<MediaStream> {
    let mut audios = dash_audio
        .into_iter()
        .filter_map(|audio| audio.into_audio_stream(AudioKind::Aac))
        .collect::<Vec<_>>();
    if let Some(flac) = flac
        && let Some(audio) = flac.audio
        && let Some(stream) = audio.into_audio_stream(AudioKind::Flac)
    {
        audios.push(stream);
    }
    if let Some(dolby) = dolby
        && let Some(audio) = dolby.audio
        && let Some(stream) = audio.into_audio_stream(AudioKind::Dolby)
    {
        audios.push(stream);
    }
    audios
}

impl StreamInfo {
    fn access_limit_message(&self) -> Option<String> {
        self.limit
            .as_ref()
            .and_then(StreamLimit::access_limit_message)
    }
}

impl StreamLimit {
    fn access_limit_message(&self) -> Option<String> {
        first_non_empty([self.msg.as_deref(), self.title.as_deref()])
    }
}

impl ViewInfo {
    fn access_limit_message(&self) -> Option<String> {
        self.dialog
            .as_ref()
            .and_then(Dialog::access_limit_message)
            .or_else(|| self.toast.as_ref().and_then(Toast::access_limit_message))
    }
}

impl Dialog {
    fn access_limit_message(&self) -> Option<String> {
        first_non_empty([self.msg.as_deref()])
    }
}

impl Toast {
    fn access_limit_message(&self) -> Option<String> {
        first_non_empty([self.text.as_deref()]).or_else(|| {
            self.toast_text
                .as_ref()
                .and_then(TextInfo::access_limit_message)
        })
    }
}

impl TextInfo {
    fn access_limit_message(&self) -> Option<String> {
        first_non_empty([self.text.as_deref()])
    }
}

struct FlvCandidate {
    quality: Option<u32>,
    description: Option<String>,
    segments: Vec<FlvSegment>,
}

fn should_replace_flv_candidate(current: Option<&FlvCandidate>, candidate: &FlvCandidate) -> bool {
    match current {
        None => true,
        Some(current) => candidate.quality.unwrap_or(0) > current.quality.unwrap_or(0),
    }
}

impl DashVideo {
    fn into_media_stream(
        self,
        stream_info: Option<&StreamInfo>,
        fallback_quality: Option<u32>,
        fallback_video_codecid: Option<u32>,
        timelength: Option<u64>,
    ) -> Option<MediaStream> {
        let base_url = self.base_url.filter(|value| !value.is_empty())?;
        let id = stream_info
            .and_then(|info| info.quality)
            .or(fallback_quality)?;
        let bandwidth = self.bandwidth.map(u64::from).or_else(|| {
            self.size.zip(timelength).and_then(|(size, time_ms)| {
                if time_ms == 0 {
                    return None;
                }
                size.checked_mul(8)?.checked_mul(1000)?.checked_div(time_ms)
            })
        });
        Some(MediaStream {
            id,
            base_url: normalize_media_url(&base_url),
            backup_urls: normalize_media_url_list(self.backup_url),
            codecs: None,
            codec_family: video_codec_family(self.codecid.or(fallback_video_codecid)),
            bandwidth,
            width: self.width.and_then(|value| u32::try_from(value).ok()),
            height: self.height.and_then(|value| u32::try_from(value).ok()),
            frame_rate: self.frame_rate.filter(|value| !value.is_empty()),
            mime_type: Some("video/mp4".to_owned()),
            size: self.size,
        })
    }
}

impl DashItem {
    fn into_audio_stream(self, kind: AudioKind) -> Option<MediaStream> {
        let base_url = self.base_url.filter(|value| !value.is_empty())?;
        Some(MediaStream {
            id: self.id?,
            base_url: normalize_media_url(&base_url),
            backup_urls: normalize_media_url_list(self.backup_url),
            codecs: audio_codec(kind, self.codecid),
            codec_family: Some(audio_codec_family(kind)),
            bandwidth: self.bandwidth.map(u64::from),
            width: None,
            height: None,
            frame_rate: None,
            mime_type: Some(kind.mime_type().to_owned()),
            size: self.size,
        })
    }
}

impl ResponseUrl {
    fn into_flv_segment(self, index: usize) -> Option<FlvSegment> {
        let url = self.url.filter(|value| !value.is_empty())?;
        Some(FlvSegment {
            order: self
                .order
                .or_else(|| u32::try_from(index + 1).ok())
                .unwrap_or(1),
            url: normalize_media_url(&url),
            backup_urls: normalize_media_url_list(self.backup_url),
            size: self.size,
            length_ms: self.length,
        })
    }
}

#[derive(Clone, Copy)]
enum AudioKind {
    Aac,
    Flac,
    Dolby,
}

impl AudioKind {
    const fn mime_type(self) -> &'static str {
        match self {
            Self::Aac | Self::Flac | Self::Dolby => "audio/mp4",
        }
    }
}

fn video_codec_family(codecid: Option<u32>) -> Option<CodecFamily> {
    match codecid {
        Some(7) => Some(CodecFamily::H264),
        Some(12) => Some(CodecFamily::Hevc),
        Some(13) => Some(CodecFamily::Av1),
        _ => None,
    }
}

fn audio_codec(kind: AudioKind, _codecid: Option<u32>) -> Option<String> {
    match kind {
        AudioKind::Aac => Some("mp4a.40.2".to_owned()),
        AudioKind::Flac => None,
        AudioKind::Dolby => Some("ec-3".to_owned()),
    }
}

const fn audio_codec_family(kind: AudioKind) -> CodecFamily {
    match kind {
        AudioKind::Aac => CodecFamily::Aac,
        AudioKind::Flac => CodecFamily::Flac,
        AudioKind::Dolby => CodecFamily::Dolby,
    }
}

fn push_stream_quality(qualities: &mut Vec<StreamQuality>, id: u32, description: Option<String>) {
    if qualities.iter().any(|quality| quality.id == id) {
        return;
    }
    qualities.push(StreamQuality { id, description });
}

fn normalize_media_url(url: &str) -> String {
    if url.starts_with("//") {
        format!("https:{url}")
    } else {
        url.to_owned()
    }
}

fn normalize_media_url_list(urls: Vec<String>) -> Vec<String> {
    urls.into_iter()
        .filter(|url| !url.is_empty())
        .map(|url| normalize_media_url(&url))
        .collect()
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[derive(Clone, PartialEq, Message)]
struct Metadata {
    #[prost(string, optional, tag = "1")]
    access_key: Option<String>,
    #[prost(string, optional, tag = "2")]
    mobi_app: Option<String>,
    #[prost(string, optional, tag = "3")]
    name: Option<String>,
    #[prost(int32, optional, tag = "4")]
    build: Option<i32>,
    #[prost(string, optional, tag = "5")]
    channel: Option<String>,
    #[prost(string, optional, tag = "6")]
    buvid: Option<String>,
    #[prost(string, optional, tag = "7")]
    platform: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
struct Device {
    #[prost(int32, optional, tag = "1")]
    app_id: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    build: Option<i32>,
    #[prost(string, optional, tag = "3")]
    buvid: Option<String>,
    #[prost(string, optional, tag = "4")]
    mobi_app: Option<String>,
    #[prost(string, optional, tag = "5")]
    platform: Option<String>,
    #[prost(string, optional, tag = "6")]
    name: Option<String>,
    #[prost(string, optional, tag = "7")]
    channel: Option<String>,
    #[prost(string, optional, tag = "8")]
    brand: Option<String>,
    #[prost(string, optional, tag = "9")]
    model: Option<String>,
    #[prost(string, optional, tag = "10")]
    osver: Option<String>,
    #[prost(string, optional, tag = "11")]
    fp_local: Option<String>,
    #[prost(string, optional, tag = "12")]
    fp_remote: Option<String>,
    #[prost(string, optional, tag = "13")]
    version_name: Option<String>,
    #[prost(string, optional, tag = "14")]
    fp: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
struct FawkesReq {
    #[prost(string, optional, tag = "1")]
    appkey: Option<String>,
    #[prost(string, optional, tag = "2")]
    env: Option<String>,
    #[prost(string, optional, tag = "3")]
    session_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
struct Network {
    #[prost(enumeration = "NetworkType", optional, tag = "1")]
    r#type: Option<i32>,
    #[prost(int32, optional, tag = "2")]
    tf: Option<i32>,
    #[prost(string, optional, tag = "3")]
    oid: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Enumeration)]
#[repr(i32)]
enum NetworkType {
    Unknown = 0,
    Wifi = 1,
    Cellular = 2,
    Offline = 3,
    Other = 4,
    Ethernet = 5,
}

#[derive(Clone, PartialEq, Message)]
struct Locale {
    #[prost(message, optional, tag = "1")]
    c_locale: Option<LocaleIds>,
    #[prost(message, optional, tag = "2")]
    s_locale: Option<LocaleIds>,
}

#[derive(Clone, PartialEq, Message)]
struct LocaleIds {
    #[prost(string, optional, tag = "1")]
    language: Option<String>,
    #[prost(string, optional, tag = "2")]
    script: Option<String>,
    #[prost(string, optional, tag = "3")]
    region: Option<String>,
}

#[cfg(test)]
pub(crate) fn test_play_view_response_frame(video_url: &str) -> Result<Vec<u8>> {
    let reply = PlayViewReply {
        video_info: Some(VideoInfo {
            quality: None,
            format: None,
            timelength: Some(123_000),
            video_codecid: None,
            stream_list: vec![
                StreamItem {
                    stream_info: Some(StreamInfo {
                        quality: Some(80),
                        format: Some("flv".to_owned()),
                        description: Some("1080P".to_owned()),
                        err_code: None,
                        limit: None,
                        need_vip: None,
                        need_login: None,
                        intact: None,
                        no_rexcode: None,
                        attribute: None,
                    }),
                    dash_video: Some(DashVideo {
                        base_url: Some(video_url.to_owned()),
                        backup_url: vec!["https://app.example/video-backup.m4s".to_owned()],
                        bandwidth: Some(1_200_000),
                        codecid: Some(12),
                        md5: None,
                        size: Some(18_000_000),
                        audio_id: Some(30280),
                        no_rexcode: None,
                        frame_rate: Some("60".to_owned()),
                        width: Some(1920),
                        height: Some(1080),
                    }),
                    segment_video: None,
                },
                StreamItem {
                    stream_info: Some(StreamInfo {
                        quality: Some(32),
                        format: Some("flv".to_owned()),
                        description: Some("FLV".to_owned()),
                        err_code: None,
                        limit: None,
                        need_vip: None,
                        need_login: None,
                        intact: None,
                        no_rexcode: None,
                        attribute: None,
                    }),
                    dash_video: None,
                    segment_video: Some(SegmentVideo {
                        segment: vec![ResponseUrl {
                            order: Some(1),
                            length: Some(123_000),
                            size: Some(10_000),
                            url: Some("https://app.example/segment.flv".to_owned()),
                            backup_url: vec!["https://app.example/segment-backup.flv".to_owned()],
                            md5: None,
                        }],
                    }),
                },
            ],
            dash_audio: vec![DashItem {
                id: Some(30280),
                base_url: Some("https://app.example/audio.m4s".to_owned()),
                backup_url: vec!["https://app.example/audio-backup.m4s".to_owned()],
                bandwidth: Some(128_000),
                codecid: None,
                md5: None,
                size: Some(2_000_000),
            }],
            dolby: Some(DolbyItem {
                r#type: Some(1),
                audio: Some(DashItem {
                    id: Some(30250),
                    base_url: Some("https://app.example/dolby.m4s".to_owned()),
                    backup_url: Vec::new(),
                    bandwidth: Some(256_000),
                    codecid: None,
                    md5: None,
                    size: Some(4_000_000),
                }),
            }),
            flac: Some(DolbyItem {
                r#type: Some(2),
                audio: Some(DashItem {
                    id: Some(30251),
                    base_url: Some("https://app.example/flac.m4s".to_owned()),
                    backup_url: Vec::new(),
                    bandwidth: Some(800_000),
                    codecid: None,
                    md5: None,
                    size: Some(12_000_000),
                }),
            }),
        }),
        business: None,
        view_info: None,
        pgc_view_info: None,
    };
    encode_grpc_frame(&reply.encode_to_vec(), false)
}

#[cfg(test)]
pub(crate) fn test_pgc_region_limit_response_frame(message: &str) -> Result<Vec<u8>> {
    let reply = PlayViewReply {
        video_info: None,
        business: None,
        view_info: Some(ViewInfo {
            dialog: Some(Dialog {
                code: Some(10001),
                msg: Some(message.to_owned()),
            }),
            toast: None,
        }),
        pgc_view_info: None,
    };
    encode_grpc_frame(&reply.encode_to_vec(), false)
}

#[cfg(test)]
pub(crate) fn test_pgc_preview_only_response_frame() -> Result<Vec<u8>> {
    let reply = PlayViewReply {
        video_info: Some(VideoInfo {
            quality: None,
            format: None,
            timelength: Some(30_000),
            video_codecid: None,
            stream_list: vec![StreamItem {
                stream_info: Some(StreamInfo {
                    quality: Some(32),
                    format: Some("dash".to_owned()),
                    description: Some("Preview".to_owned()),
                    err_code: None,
                    limit: None,
                    need_vip: None,
                    need_login: None,
                    intact: None,
                    no_rexcode: None,
                    attribute: None,
                }),
                dash_video: Some(DashVideo {
                    base_url: Some("https://app.example/preview.m4s".to_owned()),
                    backup_url: Vec::new(),
                    bandwidth: Some(300_000),
                    codecid: Some(7),
                    md5: None,
                    size: Some(1_000_000),
                    audio_id: Some(30280),
                    no_rexcode: None,
                    frame_rate: None,
                    width: Some(640),
                    height: Some(360),
                }),
                segment_video: None,
            }],
            dash_audio: Vec::new(),
            dolby: None,
            flac: None,
        }),
        business: Some(BusinessInfo {
            is_preview: Some(true),
        }),
        view_info: None,
        pgc_view_info: None,
    };
    encode_grpc_frame(&reply.encode_to_vec(), false)
}

#[cfg(test)]
mod tests {
    use super::{
        DashItem, Dialog, PlayViewReply, PlayViewReq, ResponseUrl, SegmentVideo, StreamInfo,
        StreamItem, VideoInfo, ViewInfo, decode_grpc_frame, encode_grpc_frame,
        play_view_request_body, test_play_view_response_frame,
    };
    use crate::{CodecFamily, Error};
    use prost::Message as _;

    #[test]
    fn grpc_frame_decodes_uncompressed_payload() -> anyhow::Result<()> {
        let frame = encode_grpc_frame(b"hello", false)?;
        assert_eq!(decode_grpc_frame(&frame)?, b"hello");
        Ok(())
    }

    #[test]
    fn grpc_frame_decodes_compressed_payload() -> anyhow::Result<()> {
        let frame = encode_grpc_frame(b"hello", true)?;
        assert_eq!(decode_grpc_frame(&frame)?, b"hello");
        Ok(())
    }

    #[test]
    fn play_view_request_asks_pgc_for_view_info() -> anyhow::Result<()> {
        let frame = play_view_request_body(1000, 2000, true)?;
        let payload = decode_grpc_frame(&frame)?;
        let request = PlayViewReq::decode(payload.as_slice())?;

        assert_eq!(request.ep_id, Some(1000));
        assert_eq!(request.cid, Some(2000));
        assert_eq!(request.is_need_view_info, Some(true));
        Ok(())
    }

    #[test]
    fn play_view_request_omits_view_info_for_normal_video() -> anyhow::Result<()> {
        let frame = play_view_request_body(170_001, 9988, false)?;
        let payload = decode_grpc_frame(&frame)?;
        let request = PlayViewReq::decode(payload.as_slice())?;

        assert_eq!(request.ep_id, Some(170_001));
        assert_eq!(request.cid, Some(9988));
        assert_eq!(request.is_need_view_info, None);
        Ok(())
    }

    #[test]
    fn play_view_reply_maps_to_stream_set() -> anyhow::Result<()> {
        let frame = test_play_view_response_frame("https://app.example/video.m4s")?;
        let streams = super::decode_play_view_response(&frame)?;
        assert_eq!(streams.videos[0].id, 80);
        assert_eq!(streams.videos[0].codecs, None);
        assert_eq!(streams.videos[0].codec_family, Some(CodecFamily::Hevc));
        assert_eq!(streams.videos[0].width, Some(1920));
        assert_eq!(streams.videos[0].height, Some(1080));
        assert_eq!(streams.videos[0].frame_rate.as_deref(), Some("60"));
        assert_eq!(streams.audios[0].codecs.as_deref(), Some("mp4a.40.2"));
        assert_eq!(streams.audios[0].codec_family, Some(CodecFamily::Aac));
        assert_eq!(streams.audios[1].codecs, None);
        assert_eq!(streams.audios[1].codec_family, Some(CodecFamily::Flac));
        assert_eq!(streams.audios[1].mime_type.as_deref(), Some("audio/mp4"));
        assert_eq!(streams.audios[2].codecs.as_deref(), Some("ec-3"));
        assert_eq!(streams.audios[2].codec_family, Some(CodecFamily::Dolby));
        assert_eq!(streams.flv_segments[0].order, 1);
        assert_eq!(streams.duration_seconds, Some(123));
        assert_eq!(streams.qualities[0].description.as_deref(), Some("1080P"));
        Ok(())
    }

    #[test]
    fn play_view_reply_keeps_one_flv_quality() -> anyhow::Result<()> {
        let reply = PlayViewReply {
            video_info: Some(VideoInfo {
                quality: None,
                format: None,
                timelength: Some(60_000),
                video_codecid: None,
                stream_list: vec![
                    flv_stream_item(32, "480P", "https://app.example/480.flv"),
                    flv_stream_item(64, "720P", "https://app.example/720.flv"),
                ],
                dash_audio: Vec::new(),
                dolby: None,
                flac: None,
            }),
            business: None,
            view_info: None,
            pgc_view_info: None,
        };
        let frame = encode_grpc_frame(&reply.encode_to_vec(), false)?;
        let streams = super::decode_play_view_response(&frame)?;

        assert_eq!(streams.flv_segments.len(), 1);
        assert_eq!(streams.flv_segments[0].url, "https://app.example/720.flv");
        assert_eq!(streams.accept_quality, vec![64]);
        assert_eq!(streams.qualities[0].description.as_deref(), Some("720P"));
        Ok(())
    }

    #[test]
    fn pgc_view_info_limit_maps_to_access_restricted() -> anyhow::Result<()> {
        let reply = PlayViewReply {
            video_info: None,
            business: None,
            view_info: Some(ViewInfo {
                dialog: Some(Dialog {
                    code: Some(10001),
                    msg: Some("area restricted".to_owned()),
                }),
                toast: None,
            }),
            pgc_view_info: None,
        };
        let frame = encode_grpc_frame(&reply.encode_to_vec(), false)?;
        let error = super::decode_play_view_response(&frame)
            .err()
            .ok_or_else(|| anyhow::anyhow!("PGC region-limit response should fail"))?;

        assert!(matches!(error, Error::AccessRestricted(message) if message == "area restricted"));
        Ok(())
    }

    #[test]
    fn pgc_view_info_field_9_limit_maps_to_access_restricted() -> anyhow::Result<()> {
        let reply = PlayViewReply {
            video_info: None,
            business: None,
            view_info: None,
            pgc_view_info: Some(ViewInfo {
                dialog: Some(Dialog {
                    code: Some(10001),
                    msg: Some("area restricted".to_owned()),
                }),
                toast: None,
            }),
        };
        let frame = encode_grpc_frame(&reply.encode_to_vec(), false)?;
        let error = super::decode_play_view_response(&frame)
            .err()
            .ok_or_else(|| anyhow::anyhow!("PGC field-9 region-limit response should fail"))?;

        assert!(matches!(error, Error::AccessRestricted(message) if message == "area restricted"));
        Ok(())
    }

    #[test]
    fn pgc_view_info_limit_with_audio_only_maps_to_access_restricted() -> anyhow::Result<()> {
        let reply = PlayViewReply {
            video_info: Some(VideoInfo {
                quality: None,
                format: None,
                timelength: Some(60_000),
                video_codecid: None,
                stream_list: Vec::new(),
                dash_audio: vec![DashItem {
                    id: Some(30280),
                    base_url: Some("https://app.example/audio.m4s".to_owned()),
                    backup_url: Vec::new(),
                    bandwidth: Some(128_000),
                    codecid: None,
                    md5: None,
                    size: Some(2_000_000),
                }],
                dolby: None,
                flac: None,
            }),
            business: None,
            view_info: Some(ViewInfo {
                dialog: Some(Dialog {
                    code: Some(10001),
                    msg: Some("area restricted".to_owned()),
                }),
                toast: None,
            }),
            pgc_view_info: None,
        };
        let frame = encode_grpc_frame(&reply.encode_to_vec(), false)?;
        let error = super::decode_play_view_response(&frame)
            .err()
            .ok_or_else(|| anyhow::anyhow!("audio-only PGC limit response should fail"))?;

        assert!(matches!(error, Error::AccessRestricted(message) if message == "area restricted"));
        Ok(())
    }

    #[test]
    fn pgc_preview_only_streams_map_to_access_restricted() -> anyhow::Result<()> {
        let frame = super::test_pgc_preview_only_response_frame()?;
        let error = super::decode_play_view_response(&frame)
            .err()
            .ok_or_else(|| anyhow::anyhow!("PGC preview-only response should fail"))?;

        assert!(
            matches!(error, Error::AccessRestricted(message) if message == super::APP_PREVIEW_ONLY_RESTRICTION_MESSAGE)
        );
        Ok(())
    }

    fn flv_stream_item(quality: u32, description: &str, url: &str) -> StreamItem {
        StreamItem {
            stream_info: Some(StreamInfo {
                quality: Some(quality),
                format: Some("flv".to_owned()),
                description: Some(description.to_owned()),
                err_code: None,
                limit: None,
                need_vip: None,
                need_login: None,
                intact: None,
                no_rexcode: None,
                attribute: None,
            }),
            dash_video: None,
            segment_video: Some(SegmentVideo {
                segment: vec![ResponseUrl {
                    order: Some(1),
                    length: Some(30_000),
                    size: Some(10_000),
                    url: Some(url.to_owned()),
                    backup_url: Vec::new(),
                    md5: None,
                }],
            }),
        }
    }
}
