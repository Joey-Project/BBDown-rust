[ [English](embedding.md) | 简体中文 ]

# 嵌入指南

## 范围

`bbdown_core` crate 以 `bbdown-core` package 发布，为 Rust 项目提供集成 API，涵盖类型化的
Bilibili 元数据、下载计划、媒体下载、字幕和弹幕旁路文件、二维码登录状态、批量集合解析，以及
受限区域代理诊断；调用方无需通过 shell 启动 CLI。

已发布的 `0.8.0` 包含分阶段暂存并保留历史的弹幕刷新，以及显式账号身份查询。更早发布的
`0.7.0` 增加了显式 CDN 主机池、探测和并行传输选项，以及独立的 PGC Web playurl 路由选择器。
本 source tree 还在 `0.8.0` tag 之后新增了 CDN 传输诊断；这些新增内容不在已发布的
`0.8.0` crate 中。这些网络操作仍由调用方显式选择；crate 不会根据 CLI 内置目录自动路由请求。配置应优先使用构造器和
builder 风格 API，并将元数据和 plan 结构体视为只读输出。这样 crate 增加字段时，嵌入代码更不容易
受到影响。

## 仅规划

对原始 CLI 风格输入使用 `BiliClient::plan_download`，也可以自行解析 `Input` 后调用
`BiliClient::plan`。当 UI 或 archive preflight 绑定到单独下载模式时，使用
`BiliClient::plan_download_with_mode` 或 `BiliClient::plan_with_download_mode`，这样
sidecar-only 模式不会要求解析媒体 stream。

```rust,no_run
use bbdown_core::{BiliClient, ClientConfig, Selection};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client
        .plan_download("https://www.bilibili.com/video/BV1qt4y1X7TW", Some(Selection::Current))
        .await?;

    for entry in &plan.entries {
        println!("{}: {} streams", entry.title, entry.streams.videos.len());
        println!("chapters: {}", entry.chapters.len());
    }

    Ok(())
}
```

Season 和 media 输入需要显式 `Selection`，除非输入本身已经标识某个分集。这是有意设计，
避免 library 意外规划整季内容。

## 播放请求规格

当播放器、缓存服务器或 HTTP proxy 需要选中媒体的请求数据，而不是执行文件下载时，使用
`BiliClient::plan_playback`。返回的 `PlaybackPlan` 来自和 `DownloadPlan` 相同的 resolver
路径，因此输入解析、selection、受限区域回退、intl 访问和选中流的 source reporting 会保持一致。

```rust,no_run
use bbdown_core::{BiliClient, ClientConfig, Selection};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let playback = client
        .plan_playback("BV1qt4y1X7TW", Some(Selection::Current))
        .await?;

    for entry in &playback.entries {
        for variant in &entry.variants {
            if let Some(video) = &variant.video {
                println!("{} {}", variant.id, video.url);
                println!("headers: {}", video.headers.len());
                println!("cache key: {}", video.cache_key.source_hash);
            }
        }
    }

    Ok(())
}
```

每个 `PlaybackVariant` 包含选中的 DASH 视频/音频请求规格，或 FLV 分段请求规格。
`MediaRequestSpec` 包含主 URL、备用 URL、HTTP headers、mime type、上游提供时的 exact
codec 字符串、codec-family metadata、可选音频 `language` / `language_doc` metadata、码率、
尺寸、时长、大小和 cache key。
`PlaybackVariant.selection_hints` 包含 `avplayer` profile，其中有 `playable`、`preferred`、
`score`、已知时的 exact `video_codec` / `audio_codec` 字符串、codec-family 字段、
`format_key` 和机器可读 reason code。下游客户端可以用 `PlaybackCodecPreference` 按自己的
H.264、HEVC、AV1 或其它 codec 顺序给 variants 排序，然后在交给平台播放器前验证存在的 exact
codec 字符串。cache key 会对 source URL 做 hash，
避免暴露明文，同时保留 query-string 的资源身份。`PlaybackEntry.cache_key` 标识所选内容，
`PlaybackVariant.cache_key` 组合一个可播放 variant 里的媒体 cache keys，
`PlaybackEntry.abr.groups` 按低到高 level 顺序列出 codec/mime-compatible switching
groups，而 `PlaybackVariant.abr` 会指向该 variant 所在的 group 和 level。cache server 可以
用 `MediaCacheKey` 存储已获取媒体，用 `PlaybackVariantCacheKey` 保留已完成 variants，并在
ABR policy 升级或降级时保留下层或曾访问过的兼容 level。crate 不实现播放任务状态、HLS
playlist 生成、segment serving、retention、cleanup、AVPlayer event/VOD playlist 切换或
library 注册。下游播放器和缓存服务器负责这些部分，并可把 `PlaybackPlan` 作为稳定的 HTTP
request contract。

如需 BBDown-compatible TV HTTP playurl 解析，可设置
`ClientConfig::with_playurl_mode(PlayurlMode::Tv)`；需要 mock 或代理时配置
`EndpointConfig::with_tv_api_base`；TV 端点需要账号访问时提供 `Credentials::tv_access_key`。
TV mode 当前适用于普通视频和 PGC 分集。
如需 BBDown-compatible APP gRPC playurl 解析，可设置
`ClientConfig::with_playurl_mode(PlayurlMode::App)`。普通视频 mock 或代理使用
`EndpointConfig::with_app_grpc_base`，PGC mock 或代理使用
`EndpointConfig::with_app_pgc_grpc_base`；普通视频和 PGC 默认都使用
`https://grpc.biliapi.net`。APP mode 会优先使用 Bilibili main/BALH 通用
`Credentials::access_key`，再回退到 `Credentials::tv_access_key`；如果 generic key 来自 intl OAuth，请通过
`ClientConfig::with_access_key_provider(Some(AccessKeyProvider::BiliIntlOauth2))` 告诉 core，
这样 APP mode 会优先使用 TV key，只有没有 TV key 时才回退到该通用 key。没有 provider
metadata 的旧版 credential 也会为了兼容保留 APP TV-key-first 行为。它会输出
`StreamSource::NormalApp` 或 `StreamSource::PgcApp`，并把 protobuf DASH/FLV 媒体规范化到与
HTTP modes 相同的 `StreamSet` 和 `PlaybackPlan` 表面。PGC APP gRPC 失败如果带有可识别的
restricted 或 preview-only 信号，仍会进入已配置的 restricted-area HTTP playurl proxy
fallback；但 proxy URL 只会接收通用 `Credentials::access_key`。这些信号可以来自区域限制消息、
APP permission-denied gRPC status 或 PGC response-body metadata。非零 gRPC status 会从 initial headers 和
trailing metadata 里读取。APP DASH 的分辨率和帧率 metadata 会保留到 `MediaStream` /
`PlaybackPlan` 输出。APP 数值 codec id 会暴露为 `codec_family` metadata，不会伪造 exact
MP4 codec 字符串。多个 APP legacy FLV 分段清晰度会压缩为最高质量的一组 segment，因为当前
`StreamSet` schema 把 legacy FLV 表示为单个有序 segment 列表。

## 批量和集合输入

`BiliClient::resolve_input` 接受 CLI 风格原始输入，例如 B23 短链接、`fav...`、`mid...`、
`collection...`、`series...`、`recommendations`、`history`、`watch-later`、`following`、
canonical 收藏夹 `/list/ml...` URL、path-based `/medialist/.../ml...` URL、空间合集 URL、
空间系列 URL、B 站首页、需要登录态的 `/account/history`、`/watchlater` 和
`/list/watchlater` 页面，以及动态 feed 页面。批量输入会解析为
`ResolvedContent::Collection`，其中包含完整 collection metadata 和选中的条目。
owner-scoped 空间列表 URL 会保留 uploader mid，让解析器可以使用较新的空间合集和系列
API。不带 selector 时，集合类输入会选择全部解析条目；传入 `Selection::Page(index)` 可
选择一个条目，传入 `Selection::Indices(...)` 可选择 index 列表和范围，传入
`Selection::Latest` 可选择上游列表顺序中的第一个解析条目。空集合会表示为空 item 列表，
而不是 missing-field 错误。

推荐输入使用 WEB 首页推荐端点。它支持 `recommendations`、`recommendation`、`recommend`
shorthand 和 B 站首页 URL。当前实现会输出普通视频 `av` 卡片；非视频推荐卡片会被跳过，
显式 index selection 需要时会在安全上限内继续请求后续 `fresh_idx` 刷新批次来覆盖过滤后
的普通视频卡片。

观看历史输入使用 WEB history cursor 端点，因此需要在 `ClientConfig::credentials` 中提供
cookie。当前 history collection 只输出普通视频 `archive` 记录，这些记录可以映射回普通视
频 planning 路径；PGC、直播或专栏等其它 history business 类型会被跳过，直到这些条目形态
有专门的 collection planning 支持。

稍后再看输入使用 WEB toview 端点，因此也需要在 `ClientConfig::credentials` 中提供
cookie。它支持 `watchlater`、`watch-later`、`watch_later`、`later`、`toview` 和
`https://www.bilibili.com/watchlater`、`https://www.bilibili.com/list/watchlater`，并输出该
账号稍后再看列表中的普通视频。

关注输入使用 WEB dynamic feed 端点，因此同样需要在 `ClientConfig::credentials` 中提供
cookie。它支持 `following` shorthand 和动态首页 URL。空间动态输入支持
`https://space.bilibili.com/<mid>/dynamic`。动态 feed 输入目前只输出普通视频 archive 卡
片，并跳过非视频卡片。

当前 collection 输入保留既有的 `ResolvedContent::Collection` JSON 和 Rust surface。内部现
在使用 shared feed/list selection 层，因此嵌入方可以在收藏夹、空间投稿、合集、系列、首
页推荐、观看历史、稍后再看、关注 feed 和空间动态 feed 上使用相同的 index、range、latest
和空列表语义。

普通视频可以通过显式的
`BiliClient::resolve_video_collection_membership("BV...")` 查询所属 UGC 合集/系列。视频没有
`ugc_season` 时返回 `None`，否则返回包含合集/系列类型、稳定 ID、UP 主 MID 和详情元数据的
`UgcCollectionReference`。这个查询不会改变普通 `Input::Bvid` 的解析或下载规划语义。可以把
reference 传给 `BiliClient::resolve_video_collection`，复用已有的带 UP 主范围的分页合集解析器，
得到 `VideoCollectionResolution`；解析器会使用空间合集/系列端点，不会以视频详情中内嵌的
episode 列表替代分页结果。调用这个 API 前需要先把 B23 短链接展开成最终 BV。

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, IndexSelection, IndexSelector, ResolvedContent, Selection,
};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let selection = Selection::Indices(IndexSelection::new([
        IndexSelector::index(1),
        IndexSelector::range(3, 5),
    ])?);
    let resolved = client
        .resolve_input("fav456", Some(selection))
        .await?;

    if let ResolvedContent::Collection(collection) = resolved {
        println!(
            "{} selected from {}",
            collection.selected_items.len(),
            collection.collection.title
        );
    }

    Ok(())
}
```

同一个 index selection 类型也适用于普通视频分 P 和 season 分集序号。CLI parser 接受等价
字符串，例如 `1`、`page:1`、`1,3-5` 和 `page:2-4,7`。`Selection::Episode(epid)` 仍表示
精确 PGC episode id。

下载规划会把选中的集合条目映射回普通视频条目，因此下游下载执行、归档重复检查、stream
selection、封面、字幕和弹幕旁路文件仍使用与普通视频下载相同的 API。因为 `DownloadPlan`
不暴露 collection metadata，规划阶段可以只抓取选中的批量条目集合。PUGV/cheese 分集输入
会解析为 season；当 API 报告还有更多 episode 页时会继续跟进分页，并通过
`StreamSource::PugvWeb` 规划。

## 凭据

嵌入项目可以使用 `CredentialStore`，也可以从自己的存储注入凭据。不要记录原始凭据值。
`Credentials` 的 debug 输出会脱敏，但应用日志仍应把凭据视为密钥。
默认的 `CredentialStore::load()` 和 `CredentialStore::save()` 会保留旧的单 profile JSON
文件形态。嵌入项目需要多个账号时，可以使用 `CredentialProfiles` 以及
`load_profiles`/`save_profiles`、`load_profile`/`save_profile` helper。旧扁平文件通过
profile API 读取时会表现为 `default` profile；保存命名 profile 时，会把 store 迁移到
versioned profile document，同时保留默认凭据。
`CredentialProfileSelection` 和 selected-profile store helper 提供与 CLI 相同的默认
profile / 命名 profile 路由语义，因此嵌入方可以绑定用户选择的账号，而不必重复实现迁移行为。
应用需要持久切换账号时，可以使用 `CredentialStore::set_default_profile(profile)`；它接受已存在
profile，也允许把当前默认 profile 作为 no-op。只想让单次请求覆盖所选账号时，继续使用
`CredentialProfileSelection::named(profile)`。
做增量更新时，优先使用 `CredentialStore::update_profile`、
`CredentialStore::update_selected_profile` 或 `CredentialStore::update_profiles`，不要先
`load_profiles` 再把整个旧快照 `save_profiles` 回去。这些 helper 会获取每个 store 的协作锁，
重新读取磁盘上的最新 profile document，只应用指定 mutation，然后写回私有文件，因此不会用
旧快照覆盖其它 profile 或另一个 `bbdown` 进程刚更新的 provider refresh secret。可以确认 owner
进程已退出的 stale lock file，以及旧式没有 owner-pid metadata 的 lock，会在一个较短恢复窗口后
自动接管；仍在运行或无法确认是否退出的 owner 不会被接管。每次写入都会在临时文件写入完成后、真正替换 credential
file 前，校验自己的 lock token 仍然有效；lock 获取和 stale reclaim 会用同一个 companion guard
串行化，释放 lock 前也会校验同一个 token。CLI 的自动
access-key refresh 保存路径还会先验证当前选择的 profile name 是否仍匹配本次请求的 profile，
且该 profile 是否仍匹配本次请求使用的旧 access key、access-key provider metadata、refresh
token、refresh provider 和 keypair；如果不匹配，就保持当前 store 不变。
profile document 也可以通过 `CredentialProfileMetadata` 和 `CredentialLifecycleMetadata`
携带可选 lifecycle metadata。该 metadata 会记录来源、检查时间/过期时间戳，以及是否曾有
refresh token，但不会在 metadata map 中保存原始 refresh token 值。旧 flat store 仍保持原形
态；空 metadata 在序列化 profile document 时会被省略，未知或 malformed 的可选 metadata
会在加载时被忽略，自动续期仍然属于独立策略层。
嵌入服务器需要将登录或续期绑定到同一账号时，可调用
`BiliClient::credential_account_identity(CredentialKind::Cookie)`，或选择
`AccessKey`/`TvAccessKey` 类型。成功结果包含凭据类型及官方 Web 或签名 OAuth 端点验证的
正数 `account_id`。它与健康检查不同：未登录、身份缺失、格式错误或被拒绝都会返回错误；
健康检查成功本身不证明账号身份。响应体限制为 64 KiB，OAuth 检查不转发 Web cookie，
debug/error 输出不包含凭据或账号 ID。调用方应将身份留在服务器，并在替换 profile 前比较；
库不会代替调用方授权回调或保存凭据。
已知通用 access key 的签发 keypair（用于请求签名的应用密钥对）时，可调用
`BiliClient::access_key_account_identity_with_keypair(AccessKeyIdentityKeypair::Android)`，
并选择实际的 `IntlBstar`、`BiliTv`、`Android` 或 `AndroidB`。该方法只使用指定 keypair
检查一次，而且始终读取 `Credentials::access_key`；即便选择 `BiliTv`，也不读取 TV key。
BiliTv 签名使用 `EndpointConfig::tv_passport_poll_base`，默认兼容探测回退到该 keypair
时也遵循该配置。若 TV 地址仍为默认值且只有 `passport_base` 被修改，则与现有 BiliTv
刷新路由一样使用主地址。其它通用 keypair 使用 `passport_base`。BiliTv 检查失败时不会
将该 keypair 的请求改发到其它地址；默认兼容方法只会在下述拒绝码出现时尝试下一组
keypair。
默认通用身份方法兼容缺少签发信息的旧 key：只在 API 返回 `-663` 或 `-3` 后，按上述
顺序尝试最多四组 keypair，所有尝试共享 `ClientConfig::request_timeout` 的总预算。
其它 API 拒绝、网络失败、响应过大或格式异常，以及缺失或非正数账号 ID 都立即终止。
provider 名称或 refresh-keypair 元数据本身不能证明当前 key 的实际签发者。Cookie 与 TV
身份检查仍使用各自独立的凭据和原有请求路径，健康检查行为也保持不变。
这些新增 API 包含在 `0.8.0` 开发线中，`0.7.0` crate 不提供。
对于二维码登录，如果下游应用需要稳定的可序列化扫码 URL 和 `qr_payload`，可以把
`QrLoginTicket` 转换成 `QrLoginTicketOutput`；当前 WEB 和 TV 登录流程会直接使用扫码 URL
作为 QR payload。兼容方法 `poll_web_qr_login` / `poll_tv_qr_login` 会返回
`QrLoginState::Succeeded { credentials: Credentials }`。当存储层需要 refresh metadata 时，调用
`poll_web_qr_login_credentials` / `poll_tv_qr_login_credentials` 获取
`QrLoginCredentials`，其中包含运行时 `Credentials` 和可选 refresh metadata。自行管理存储的嵌入方应把运行时
credential 与 refresh token 分开持久化：WEB refresh token 使用
`CredentialProfileSecrets::set_cookie(CredentialRefreshSecret::default().with_refresh_token(...))`
保存，TV refresh token 使用 `set_tv_access_key(...)` 保存；lifecycle metadata 只记录
`refresh_token_present`，以及从 `expires_in` 派生出的过期时间。
对于通用 access-key 授权，`AccessKeyLoginConfig::biliplus(callback_origin)` 会构造
BiliPlus/BALH-compatible browser handoff URL；`AccessKeyLoginTicketOutput::qr_payload` 可以
直接渲染成二维码。parser 接受历史 `balh-login-credentials:` message shape，payload 可以是
JSON，也可以是 URL/query callback；返回的 `AccessKeyLoginCredentials` 包含通用
`access_key` 以及可选的 refresh/expiration metadata。调用 `credentials()` 只会把通用
access key 转成现有 `Credentials` model，因此自行管理存储的嵌入应用需要显式保存来自
`oauth_expires_at`、`expires_at`、`expires_in` 和 `refresh_token` 的 lifecycle metadata。
refresh secret 应与运行时 `Credentials` 分开保存：使用 `CredentialProfileSecrets`，在产出当前
access key 的 provider 下保存 `AccessKeyProviderSecret`。CLI 会把 BiliPlus/BALH callback
写入 `balh_biliplus` provider，并把 refresh provider 标记为 `bilibili_main_oauth2`、keypair
family 标记为 `bili_tv`。CLI 登录路径会把 lifecycle metadata 记录到当前选择的 credential
profile：绝对 expiry 字段会直接保存，相对 `expires_in` 会按获取时间换算，refresh token 只记录
是否存在，不会把 token 值复制进 lifecycle metadata。
在 browser `postMessage` flow 中，应优先使用
`AccessKeyLoginTicketOutput::credentials_from_message(event_origin, data)`，让 sender origin
先按 ticket 的可信 auth origin 或 callback origin 校验后再解析。只有当嵌入应用已经自行验证
message provenance 时，才直接使用 raw `AccessKeyLoginCredentials::from_balh_*` parser。
access-key lifecycle orchestration 可以在加载
`CredentialProfiles::profile_lifecycle_status(...)` 后调用
`AccessKeyRenewalDecision::from_profile_status(profile_status, force_reauthorization)`。`NoAction`
decision 表示当前所选 profile 的 access-key metadata 在调用方 policy 下仍是 fresh；
`Reauthorize` 表示 UI 应先尝试 provider-specific refresh，或渲染新的
`AccessKeyLoginTicketOutput` 并收集下一次 BALH callback。decision 中的
`automatic_refresh_readiness` 是刻意显式的：`metadata_only_refresh_token` 表示上一次
callback 在 provider secret 保存能力出现前报告过 refresh token；`ready` 表示当前所选 profile
有 provider-scoped refresh secret、refresh provider，以及该 provider 进行网络 refresh 所需的
keypair。嵌入方可以用已保存的 access key 和匹配的 `AccessKeyProviderSecret` 构造
`AccessKeyRefreshRequest`，再调用 `BiliClient::refresh_access_key(...)`。client 支持通过
`EndpointConfig::passport_base` 进行 Bilibili main OAuth2 refresh，也支持通过
`EndpointConfig::intl_passport_base` 进行 BiliIntl OAuth2 refresh；`bili_tv` main-provider
keypair 会路由到 TV OAuth refresh path。成功后会返回新的 `AccessKeyLoginCredentials`，
调用方可以复用与首次 access-key login 相同的 lifecycle/secret
持久化路径。网络或 API refresh 失败应视为 non-destructive：保留旧 credential，并在策略需要
用户介入时回退到重新授权 UI。
WEB cookie 和 TV token refresh 是独立 primitive，因为它们不是 provider-scoped generic access
key。嵌入方可以用已保存 cookie 和对应 refresh token 构造 `WebCookieRefreshRequest`，再调用
`BiliClient::refresh_web_cookie(...)`；client 会检查
`/x/passport-login/web/cookie/info`，派生 RSA-OAEP/SHA-256
`correspond/1/{path}` challenge，从
`EndpointConfig::web_base` 提取 `refresh_csrf`，调用 cookie refresh endpoint，合并
`Set-Cookie` header，并确认旧 refresh token。如果 Bilibili 表示现有 cookie 暂时不需要
refresh，返回的 `WebCookieRefreshCredentials::refreshed` 会是 `false`；嵌入方应把它当成
no-op，而不是重写 credential 或 acquisition metadata，但记录新的 checked timestamp 有助于后续
preflight 决策。用已保存 TV token 和 refresh token 构造
`TvAccessKeyRefreshRequest` 后调用 `BiliClient::refresh_tv_access_key(...)`；它会走 TV OAuth
refresh endpoint；配置了 TV passport 覆盖时会使用 `EndpointConfig::tv_passport_poll_base`，
否则保留 main `EndpointConfig::passport_base` 以兼容现有 main OAuth refresh 部署。它会返回
带运行时 `tv_access_key`、可选 refresh token 和过期 metadata 的
`TvAccessKeyLoginCredentials`。这些 refresh 调用不会修改存储；嵌入方应在确认请求仍匹配当前
选择账号后，再保存返回的 credential、lifecycle metadata 和 `CredentialRefreshSecret`。
当嵌入项目需要在决定提示登录、导入 token 或继续匿名请求前做脱敏诊断时，可以调用
`BiliClient::check_credential_health()`。报告会分别包含 WEB cookie、通用 `access_key` 和
TV `tv_access_key` 的 probe；`kind` 表示凭据槽位，`scope` 表示实际检查的消费场景。通用
`access_key` probe 当前只覆盖 intl/Bstar OAuth-info scope，因此如果下游需要 APP gRPC 或
proxy-specific assurance，应把它作为单独策略判断。probe message 在序列化前会先脱敏。使用
`CredentialHealthReport::summary()` 可以得到适合 JSON/UI 展示的汇总状态；当 UI 或 preflight
policy 只需要某一个检查时，使用 `CredentialHealthReport::probe(kind, scope)`。
profile document 也可以在不发网络请求的情况下通过
`CredentialProfiles::profile_lifecycle_status(profile, policy)` 或
`CredentialProfiles::lifecycle_statuses(policy)` 做 lifecycle 评估。`CredentialLifecyclePolicy`
要求调用方显式传入 `now_unix_millis`，方便 embedding app 在 UI、后台任务和测试中得到确定
的 stale/expiring 结果。
plan/download preflight 可以用当前所选 profile 的 lifecycle status 和 media request context
构造 `CredentialPreflightReport`。`CredentialPreflightReport::from_client_context(...)` 是保守
的 client-config 形式；`CredentialPreflightReport::from_media_request_context(...)` 允许
embedding app 对不会使用 PGC proxy fallback 的输入跳过 restricted-area proxy requirement。
当 resolved source 没有 WEB/TV/APP playurl path 时，例如 intl/Bstar 输入只应该检查 intl 通用
`access_key`，请使用 `CredentialPreflightReport::from_media_paths_context(...)`。这些形式都会对齐
client 实际会发送的 credential：WEB playurl 的 cookie 是 optional；TV
playurl 要求 `tv_access_key`；APP playurl 接受 `tv_access_key` 或通用 `access_key` 任一可用，
并按 provider metadata 决定两者都存在时的顺序：Bilibili main/BALH generic key 先于 TV key；
`bili_intl_oauth2` key 和没有 provider metadata 的旧版 profile 会让位给 TV key。stale optional WEB playurl cookie 只会产生 warning，
不会成为 blocker，因此公开视频可以继续以匿名路径运行。history、watch-later 和 following
这类账号级 feed 输入会在选择 media stream 前访问已认证 WEB API，应额外加入
`CredentialPreflightRequirement::authenticated_web_api_cookie()`；公开的 space dynamic 页面可以匿名访问，
不应加入这个 required-cookie preflight。restricted-area proxy fallback
会把通用 `access_key` 视为 optional：已存在的 key 会被检查并可能由 resolver 转发，缺失 key 不会阻断
自带认证或允许匿名 fallback 的 proxy URL。intl/Bstar episode 的 media 和 subtitle path 会要求官方
intl metadata、playurl 和 subtitle 请求实际使用的通用 `access_key`。cover-only 和 danmaku-only 的
intl episode path 应跳过这个 access-key requirement，因为它们只需要 metadata 和 sidecar endpoint；
如果 profile 里存在 access key，metadata 请求仍可带上它。
intl/Bstar 和 PUGV/cheese 这类固定来源输入不应继承调用方的全局 TV/APP playurl credential
requirement，sidecar-only mode 也应跳过 media-stream preflight。
这个 report 是纯值：它会列出 requirement status、warning/blocker，以及当前所选
profile 的 `AccessKeyRenewalDecision`，但不会修改 credential storage。embedding app 如果接受短链，
应该先用 `BiliClient::parse_input(...)` 规范化输入，再判断 PGC proxy fallback 或 intl access-key
preflight 是否可能运行。嵌入项目可以把 blocker 作为 fail-fast UI，把 warning 作为非阻断 banner，或在
`should_attempt_access_key_renewal()` 为 true 时调用 `BiliClient::refresh_access_key(...)`，
并通过自己的存储层保存刷新后的 credential。该 renewal predicate 会要求先补齐缺失的非
access-key credential；但已存在且 lifecycle metadata 为 stale、expiring、expired 或 unknown
的非 access-key credential 不会阻止 refresh-ready 的通用 access key 刷新。
如果当前 requirement 选择的是 WEB cookie 或 TV `tv_access_key`，嵌入方可以从
`CredentialLifecycleCredentialStatus` 做同类策略判断：credential 已存在、状态不是 fresh、来源是
WEB/TV QR login，并且 `refresh_token_secret_present == Some(true)` 时，可先调用匹配的 WEB/TV
refresh primitive，再重试 media request。计算 lifecycle
status 和脱敏 presence 布尔值时，只含空白字符的已保存 credential 字符串会按 missing 处理。
request builder 会在使用前 trim 已保存 credential；trim 后为空的值不会写入请求。

```rust,no_run
use bbdown_core::{
    AccessKeyLoginConfig, AccessKeyLoginCredentials, AccessKeyLoginTicketOutput, BiliClient,
    ClientConfig, CredentialKind, CredentialLifecyclePolicy, CredentialLifecycleStatus,
    CredentialPreflightMode, CredentialPreflightReport, CredentialProfileSelection,
    CredentialStore, Credentials, PlayurlMode, RestrictedAreaConfig,
};

async fn check_credentials() {
    let credentials = Credentials::default()
        .with_cookie("SESSDATA=...")
        .with_access_key("...");

    let config = ClientConfig::default().with_credentials(credentials);
    let client = BiliClient::new(config);
    let health = client.check_credential_health().await;
    let _summary = health.summary();
}

fn load_named_profile(store: &CredentialStore, profile: &str) -> bbdown_core::Result<Credentials> {
    let selection = CredentialProfileSelection::named(profile)?;
    store.load_selected_profile(&selection)
}

fn access_key_lifecycle_status(
    store: &CredentialStore,
    profile: &str,
    now_unix_millis: u64,
) -> bbdown_core::Result<Option<CredentialLifecycleStatus>> {
    let profiles = store.load_profiles()?;
    let policy = CredentialLifecyclePolicy::at_unix_millis(now_unix_millis);
    let status = profiles.profile_lifecycle_status(profile, &policy)?;
    Ok(status
        .credential_statuses
        .into_iter()
        .find(|status| status.kind == CredentialKind::AccessKey)
        .map(|status| status.status))
}

fn plan_preflight_report(
    store: &CredentialStore,
    profile: &str,
    now_unix_millis: u64,
) -> bbdown_core::Result<CredentialPreflightReport> {
    let profiles = store.load_profiles()?;
    let policy = CredentialLifecyclePolicy::at_unix_millis(now_unix_millis);
    let status = profiles.profile_lifecycle_status(profile, &policy)?;
    Ok(CredentialPreflightReport::from_client_context(
        CredentialPreflightMode::Warn,
        &status,
        PlayurlMode::Web,
        &RestrictedAreaConfig::default(),
    ))
}

fn access_key_login_ticket() -> bbdown_core::Result<AccessKeyLoginTicketOutput> {
    let config = AccessKeyLoginConfig::biliplus("https://www.bilibili.com")?;
    Ok(config.ticket()?.output())
}

fn access_key_from_balh_message(
    ticket: &AccessKeyLoginTicketOutput,
    event_origin: &str,
    message: &str,
) -> bbdown_core::Result<Credentials> {
    Ok(ticket
        .credentials_from_message(event_origin, message)?
        .credentials())
}

fn access_key_from_trusted_payload(message: &str) -> bbdown_core::Result<Credentials> {
    Ok(AccessKeyLoginCredentials::from_balh_message(message)?.credentials())
}
```

## 受限区域 PGC 规划

crate 不会内置公共代理默认值。只配置你自己运营或信任的代理主机。受限区域回退只会针对
PGC playurl 区域错误尝试，而不是针对任意官方 API 失败。

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, RestrictedArea, RestrictedAreaConfig, RestrictedAreaProxy, Selection,
};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let restricted_area = RestrictedAreaConfig::default()
        .with_area_hint(RestrictedArea::Hk)
        .with_proxy(RestrictedAreaProxy::playurl(
            "https://proxy.example/playurl",
            Some(RestrictedArea::Hk),
        ))
        .with_proxy(RestrictedAreaProxy::bilibili_api(
            "https://your-server.example/bili/api",
            Some(RestrictedArea::Hk),
        ));

    let client = BiliClient::new(ClientConfig::default().with_restricted_area(restricted_area));
    let plan = client.plan_download("ep664928", Some(Selection::Current)).await?;

    println!("planned {} entries", plan.entries.len());
    Ok(())
}
```

回退成功时，条目会报告 `StreamSource::PgcProxy` 并包含解析器诊断。诊断端点会压缩到
origin，诊断消息会脱敏常见密钥模式。
上面的 API-path 示例可指向你自建的 BiliRoaming-compatible 服务：客户端请求其
`/pgc/player/web/playurl` 路由，并发送 `area`、`ep_id` 和可选的通用 `access_key`。crate 不
内置公共解析服务器，也不保证账号授权或区域可用性；服务返回的签名媒体 URL 会原样进入
plan，供下游下载器或播放器使用。

嵌入式应用若要并发尝试官方与反代的 PGC **Web playurl**，可用同一份配置创建两个 client。
`PgcWebPlayurlRoute::OfficialOnly` 只请求官方 playurl；`ProxyOnly(proxy)` 只请求指定反代
服务器（API 风格反代可能依次尝试该服务器的 Web 与 Web v2 路径）。默认的
`OfficialThenProxy` 保持现有的区域错误回退链。这些模式不改变 PGC metadata 获取，也不
影响 TV/APP playurl。

```rust,no_run
use bbdown_core::{BiliClient, ClientConfig, PgcWebPlayurlRoute, RestrictedAreaConfig, RestrictedAreaProxy};

# async fn example() -> bbdown_core::Result<()> {
let selected = RestrictedAreaProxy::playurl("https://proxy.example/playurl", None);
let config = ClientConfig::default()
    .with_restricted_area(RestrictedAreaConfig::default().with_proxy(selected.clone()));
let official = BiliClient::new(config.clone()
    .with_pgc_web_playurl_route(PgcWebPlayurlRoute::OfficialOnly));
let proxy = BiliClient::new(config
    .with_pgc_web_playurl_route(PgcWebPlayurlRoute::ProxyOnly(selected)));
let (official_result, proxy_result) = tokio::join!(
    official.plan_playback("ep664928", None),
    proxy.plan_playback("ep664928", None),
);
# let _ = (official_result, proxy_result);
# Ok(())
# }
```

调用方负责选取胜出结果，核对 `PlaybackEntry` 的内容标识和 `source`，并通过脱敏后的
`diagnostics` 识别反代 origin 和区域。proxy-only 失败时不会悄悄切换到官方 playurl。
指定反代可使用通用 `Credentials::access_key`，但不会收到 Bilibili Web cookie。丢弃规划
future 即可取消进行中的请求；core 不会在后台继续启动解析竞速任务。

## 下载执行

下载是显式动作。library 默认禁用 mux，因此嵌入应用不会在未选择的情况下启动 `ffmpeg`。
当选中的上游播放器 metadata 暴露可用章节边界时，plan 条目可能包含 `ChapterTrack`。通过
`MuxOptions::ffmpeg(...)` 启用 mux 时，这些章节会以临时 ffmetadata 交给 ffmpeg，返回的
`MuxReport::chapter_count` 会记录写入的章节数量。禁用 mux 时，章节只保留为 plan 条目上的
metadata，不会写出章节 sidecar。

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DanmakuFormat, DownloadMode, DownloadOptions, DownloadPathTemplates,
    MuxOptions, RetryPolicy, StreamSelection, SubtitleAiPolicy,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let options = DownloadOptions::new("downloads")
        .with_stream_selection(StreamSelection::video(80))
        .with_download_mode(DownloadMode::All)
        .with_retry_policy(RetryPolicy::new(3, Duration::from_millis(250)))
        .with_download_idle_timeout(Some(Duration::from_secs(30)))
        .with_cover(true)
        .with_subtitles(true)
        .with_subtitle_ai_policy(SubtitleAiPolicy::PreferNonAi)
        .with_danmaku(true)
        .with_danmaku_format(DanmakuFormat::Ass)
        .with_path_templates(
            DownloadPathTemplates::new()
                .with_output_dir("{title}-{entry_count:02}")
                .with_entry_dir("{index:03}-{entry_title}-{content_id}")
                .with_mux_file_stem("{index:03}-{entry_title}"),
        )
        .with_mux(MuxOptions::ffmpeg("ffmpeg"));

    let report = client
        .download_input("BV1qt4y1X7TW", None, options)
        .await?;

    println!("wrote {} entries", report.entries.len());
    Ok(())
}
```

嵌入应用需要进度 callback、但不想解析 CLI 输出时，使用 `*_with_progress` 下载方法。
`DownloadProgressEvent` 会覆盖 plan 开始/完成/失败/取消、条目开始/完成/失败、文件
开始/chunk/完成/失败、传输字节和传输诊断事件，以及 mux 开始/完成/失败。callback 是同步调用，
应保持轻量；如果 UI 更新或数据库写入可能阻塞下载任务，请把事件转发到应用自己的 channel。

`0.8.0` 发布后的 source tree 新增了 `DownloadProgressEvent::TransferBytesReceived` 和
`DownloadProgressEvent::TransferDiagnostic`。每消费一个 `bytes_stream()` 响应体 chunk，都会在
通过状态码和 header 检查后、响应体长度校验及写盘前发出字节事件；未读取就被拒绝的响应体计为 0。计数受调用方客户端透明内容
解码配置影响，因此它不是压缩实体字节数或全部网络传输流量。统计已消费的响应体字节时应累加
`bytes_delta`；
`bytes_received` 是该请求的累计值，不能再次累加。它与 `FileProgress.bytes_written` 含义不同。
字节事件的 `request_id` 在一次文件传输操作内从 1 开始，不要仅按 ID
跨文件或独立探测调用合并请求。操作级诊断的 request ID 和 phase 可以为空。`phase` 区分自动探测、
分片预检、Range 分片、整文件请求和独立探测。
事件只带解析出的 host 和可选文件上下文，不包含请求 URL。诊断事件使用类型化信息报告候选选中/排除、
计划重试、整文件回退和请求结束，不包含上游错误文本。范围包括文件响应（媒体和旁路文件）及
CDN 探测/Range 请求，不包括 metadata、playurl 或 authentication API 请求。候选选中仅表示通过
兼容性预检，不能证明整个表示的内容相同。分片候选选中/排除会在全部候选探测结束后作为操作级诊断发出
（`request_id: None`、`phase: ShardProbe`），不会复用已发出 `RequestFinished` 的探测请求 ID。即使没有
可兼容的候选组、传输随后回退，phase 仍是分片探测。host label 只包含解析出的 host 和可选端口，不包含
URL path、query 或 credentials。探测超时使用 `TimedOut`；续传响应体长度按剩余请求长度分类；span/探测
失败使用 `ProbeFailed`，只有显式总大小不匹配才使用 `SizeMismatch`。
仅 scheme 不同的候选使用 `SchemeMismatch`（`scheme_mismatch`）；只有实际 path 或 query 不同时才使用
`PathQueryMismatch`（`path_query_mismatch`）。这些 reason 只描述现有的 same-scheme 兼容策略，不会改变
候选兼容规则。对于整文件请求，成功的 `RequestFinished` 会先于 `FileCompleted` 发出；只有响应体已读完并
flush、长度校验通过且必要的文件替换成功后，才会发出成功结果。HTTP 416 表示续传内容已完整时，也会走
相同的完成出口，避免重复成功事件。以失败或取消结束的文件操作不会发出 `FileCompleted`；取消或 drop 仍可能没有
请求终态诊断。
取消或 drop 的请求可能没有终态结果，但之前发出的字节增量仍有效。这些 API 是已发布 `0.8.0`
tag 后的 source additions，不属于已发布 crate。

progress sink 默认启用这些事件；可重载 `wants_transfer_diagnostics()` 返回 `false` 关闭它们，
`NoopDownloadProgress` 已关闭该功能。下面的字段处理不假设操作级诊断一定包含 request ID 或 phase：

```rust,no_run
use bbdown_core::DownloadProgressEvent;

fn inspect_transfer_event(event: &DownloadProgressEvent) {
    match event {
        DownloadProgressEvent::TransferBytesReceived {
            request_id,
            phase,
            host,
            bytes_delta,
            bytes_received,
            ..
        } => eprintln!("request={request_id} phase={phase:?} host={host:?} +{bytes_delta} ({bytes_received})"),
        DownloadProgressEvent::TransferDiagnostic {
            request_id,
            phase,
            host,
            diagnostic,
            ..
        } => eprintln!("request={request_id:?} phase={phase:?} host={host:?} {diagnostic:?}"),
        _ => {}
    }
}
```

独立探测可调用 `probe_media_cdns_with_progress`，参数与旧 `probe_media_cdns` 相同，另加 progress
sink：

此 API 在已发布的 `0.8.0` tag 之后加入。编译下面的示例时，请使用包含该改动的源码 checkout；
crates.io 上的 `0.8.0` package 不包含此 API。

```rust,no_run
use bbdown_core::{
    probe_media_cdns_with_progress, BiliClient, DownloadProgressSink, MediaHostOptions,
    MediaStream, Result,
};

async fn probe(
    client: &BiliClient,
    stream: &MediaStream,
    progress: &impl DownloadProgressSink,
) -> Result<()> {
    let _results = probe_media_cdns_with_progress(
        client,
        stream,
        &MediaHostOptions::default(),
        progress,
    )
    .await?;
    Ok(())
}
```

独立探测事件的 entry/file context 均为 `None`，每次调用的 request ID 从头开始。旧
`probe_media_cdns` 仍可使用，且不会产生传输事件。

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DownloadOptions, DownloadProgressEvent, DownloadProgressSink,
};

struct ProgressLogger;

impl DownloadProgressSink for ProgressLogger {
    fn on_download_progress(&self, event: &DownloadProgressEvent) {
        eprintln!("{event:?}");
    }
}

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let progress = ProgressLogger;
    let report = client
        .download_input_with_progress(
            "BV1qt4y1X7TW",
            None,
            DownloadOptions::new("downloads"),
            &progress,
        )
        .await?;

    let summary = report.summary();
    println!(
        "wrote {} files across {} entries ({} bytes newly written)",
        summary.file_count, summary.entry_count, summary.bytes_written
    );
    Ok(())
}
```

Archive-aware flow 也有对应的 `download_plan_with_archive_decision_with_progress` 和
`download_plan_with_archive_preflight_decision_with_progress` 方法。已有非 progress 方法仍然
可用，并会使用 no-op sink。把 `plan_completed`、`plan_failed` 和 `plan_cancelled` 视为一
次下载任务互斥的终态。显式 archive duplicate 取消和通过 `DownloadCancellationToken` 取
消的下载都会发出 `plan_cancelled`。由 token 触发的取消会返回 `Error::Cancelled`；UI 需要
区分用户停止和普通失败时，可以使用 `Error::is_cancelled()`。显式 archive duplicate 取消是
preflight decision 路径，调用方应使用 duplicate decision/report 状态，而不是把每个
`plan_cancelled` 都当成 `Error::Cancelled`。

```rust,no_run
use bbdown_core::DownloadProgressEvent;

fn apply_progress(event: &DownloadProgressEvent) {
    match event {
        DownloadProgressEvent::FileProgress {
            path,
            bytes_written,
            expected_size,
            ..
        } => {
            eprintln!("{path:?}: {bytes_written}/{expected_size:?}");
        }
        DownloadProgressEvent::PlanCompleted { .. } => eprintln!("download completed"),
        DownloadProgressEvent::PlanFailed { error, .. } => eprintln!("download failed: {error}"),
        DownloadProgressEvent::PlanCancelled { error, .. } => {
            eprintln!("download cancelled: {error}");
        }
        _ => {}
    }
}
```

UI、任务 supervisor 或 cache server 需要显式停止下载时，使用
`*_with_cancellation` 或 `*_with_progress_and_cancellation` 下载变体。同一个 token 可以从任
意 task 调用 `cancel()` 或 `cancel_with_reason(...)` 取消。执行层会在规划、开始新条目和
sidecar 前、等待 retry backoff 时、发送 HTTP request 时、流式读取 response body 时，以及
等待 `ffmpeg` mux 时检查取消。取消时，新创建的部分文件会被删除；续传文件会截断回本次
尝试前的大小；已经完成的条目保持有效，并会计入终态事件。

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DownloadCancellationToken, DownloadOptions, DownloadProgressEvent,
    DownloadProgressSink,
};

struct UiProgress;

impl DownloadProgressSink for UiProgress {
    fn on_download_progress(&self, event: &DownloadProgressEvent) {
        // Forward the event into the application's UI or task-state channel.
        eprintln!("{event:?}");
    }
}

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let progress = UiProgress;
    let cancellation = DownloadCancellationToken::new();

    let cancel_from_ui = cancellation.clone();
    tokio::spawn(async move {
        // Replace this with the application's cancel button, task shutdown, or HTTP disconnect.
        wait_for_user_cancel().await;
        cancel_from_ui.cancel_with_reason("user cancelled download");
    });

    let result = client
        .download_input_with_progress_and_cancellation(
            "BV1qt4y1X7TW",
            None,
            DownloadOptions::new("downloads"),
            &progress,
            &cancellation,
        )
        .await;

    if let Err(error) = &result {
        if error.is_cancelled() {
            eprintln!("download stopped by caller: {error}");
        }
    }

    result.map(|_| ())
}

async fn wait_for_user_cancel() {}
```

当下游任务需要稳定 UI 状态、显式取消、首选音频和字幕筛选时，可以组合使用同一组配置表面：

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DownloadCancellationToken, DownloadOptions, DownloadProgressEvent,
    DownloadProgressSink, StreamSelection, SubtitleAiPolicy,
};

struct TaskProgress;

impl DownloadProgressSink for TaskProgress {
    fn on_download_progress(&self, event: &DownloadProgressEvent) {
        match event {
            DownloadProgressEvent::PlanCompleted { .. }
            | DownloadProgressEvent::PlanFailed { .. }
            | DownloadProgressEvent::PlanCancelled { .. } => {
                eprintln!("terminal task state: {event:?}");
            }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let cancellation = DownloadCancellationToken::new();
    let options = DownloadOptions::new("downloads")
        .with_stream_selection(StreamSelection::audio_language("Japanese"))
        .with_subtitles(true)
        .with_subtitle_ai_policy(SubtitleAiPolicy::PreferNonAi);

    let report = client
        .download_input_with_progress_and_cancellation(
            "BV1qt4y1X7TW",
            None,
            options,
            &TaskProgress,
            &cancellation,
        )
        .await?;

    let summary = report.summary();
    eprintln!("{} files, {} bytes", summary.file_count, summary.total_bytes);
    Ok(())
}
```

当 UI 需要展示质量选择时，先使用 `bbdown plan` 或 `BiliClient::plan_download`。
`StreamSelection::video`、`StreamSelection::audio` 和 `StreamSelection::new` 会从 plan 中
选择精确的 DASH stream id。`StreamSelection::audio_language("ja-JP")` 或
`StreamSelection::new(None, Some(30280)).with_audio_language("Japanese")` 会选择第一条
`MediaStream.language` 或 `language_doc` 与请求值大小写不敏感匹配的 DASH 音频流。显式
stream selection 会写入 archive content key，因此不同清晰度或音频语言不会互相满足 duplicate
preflight。
嵌入调用方需要单独输出某一种文件时，使用 `DownloadMode::VideoOnly`、`AudioOnly`、
`SubtitleOnly`、`DanmakuOnly` 或 `CoverOnly`。sidecar-only 模式不要求媒体 stream，且不
会启动 mux；video-only 和 audio-only 模式只选择对应 DASH stream。当后续 download
options 使用非默认 mode 时，应先用 mode-aware planning API 再调用
`DownloadPreflight::inspect`。
`DownloadPlan` 会通过 `SubtitleTrack::is_ai_generated`、`ai_type` 和 `ai_status` 保留原始
字幕 AI metadata。使用 `DownloadOptions::with_subtitle_ai_policy(...)` 可以保留全部字幕、
在同语言存在非 AI 字幕时优先非 AI 字幕、排除 AI 字幕，或只下载 AI 字幕。非默认 subtitle
AI policy 会参与 archive key，因为它会改变旁路文件集合。
弹幕旁路文件默认使用 `DanmakuFormat::Xml`；当嵌入 UI 需要 ASS-only 输出时，使用
`DanmakuFormat::Ass`；需要同时保留 XML 和 ASS 时，使用
`DownloadOptions::with_danmaku_formats([DanmakuFormat::Xml, DanmakuFormat::Ass])`。
`DownloadPathTemplates` 可定制输出根目录、条目目录和 mux 后文件名 stem，同时保持媒体和
sidecar 文件名稳定。模板字符串会渲染为一个路径组件，并在展开后清洗。输出模板可使用
`{title}` 和 `{entry_count}`；条目和 mux 模板可使用 `{title}`、`{entry_title}` 或
`{page_title}`、`{index}` 或 `{page}`、`{aid}`、`{bvid}`、`{cid}`、`{epid}` 和
`{content_id}`。数字 placeholder 支持 `{index:03}` 这样的补零格式。条目模板必须为每个选
中的条目渲染出唯一目录名；标题可能重复时请加入 `{index}` 或 `{content_id}`。如果嵌入应
用会先展示 archive preflight 结果再下载，请用后续执行时相同的 `DownloadOptions` 和模板
构建 preflight。

crate 默认会原样保留 plan 中的媒体 URL。嵌入应用如果需要 BBDown-like 的 PCDN 规避，或
需要指定自定义 UPOS host，可以在 `DownloadOptions` 上设置 `MediaHostOptions`。该策略只
应用于 DASH 和 FLV 媒体候选；封面、字幕和弹幕旁路 URL 不会被改写。

```rust,no_run
use bbdown_core::{DownloadOptions, MediaHostOptions};

let options = DownloadOptions::new("downloads").with_media_hosts(
    MediaHostOptions::bbdown_cli_default()
        .with_upos_host("upos-sz-mirrorcoso1.bilivideo.com"),
);
```

可以组合以下 builder 配置有序 CDN host pool、可选 Range 测速和并行媒体传输：

```rust,no_run
use bbdown_core::{DownloadOptions, MediaHostOptions};

let options = DownloadOptions::new("downloads")
    .with_media_hosts(
        MediaHostOptions::new().with_cdn_hosts(["edge-a.example", "edge-b.example"]),
    )
    .with_cdn_probe(true)
    .with_cdn_parallelism(4);
```

较大的 host 池可以调用 `MediaHostOptions::with_original_after_cdn_hosts(1)`，使 donor 的
常规媒体候选排在第一个指定 CDN 之后、其余 host 之前。它仍遵守已有的 host 替换策略；默认
行为会把该候选放在整个手动配置池之后。测速可能重排前 8 个候选（包括 donor），但不会一次
探测完整地区目录。

测速和并行传输只作用于 DASH/FLV 媒体，不会改动旁路文件 URL。并行度默认是 1（关闭），2 到 8
会启用并行传输；API 配置非法值会在下载计划校验或执行时明确报错。测速和分片请求要求服务端
返回有效的 HTTP Range 响应且总长度一致。计划未提供大小时，开启测速或并行传输会先用有界的
`bytes=0-0` 请求发现大小。已有非空 partial 文件仍走常规续传路径；符号链接和有多个硬链接的目标
走常规下载路径，且续传时不会因测速改变候选顺序。特殊文件目标也走常规路径。Unix 上的
no-resume 新下载可以对已有单链接普通文件分片；非 Unix 平台对已有文件保守地使用常规路径。
分片只会混用前缀样本相同、
URL scheme/path/query 相同的候选；每个分片从选定 CDN 获取一次，失败时再向其他候选重试。
并行度为 8 时最多同时发出 8 个 Range 请求。这样基准 CDN 不必再承担整份文件的验证传输，
但前缀和大小检查无法证明不同 host 的后续字节完全一致；若边缘节点提供不同版本，完成的文件可能
无声地混入不同版本的分片。测速和并行传输不保证一定提速；
改写 host 也不能证明签名 URL 一定能被另一 CDN 接受。完整分片文件成功发布后，
`DownloadProgressEvent::CdnShardCompleted` 才会报告各分片的来源 host 和字节数；被丢弃的暂存
字节不会报告为进度。事件不含签名 URL 的路径与 query；按 host 汇总即可核对最终文件的供数分布。

嵌入应用可以通过 `probe_media_cdns(client, stream, media_hosts)` 探测当前已签名的媒体表示。
它对最多 8 个候选分别读取不超过 64 KiB，返回只含 host 的标签、成功状态、耗时、吞吐量和
总大小（`CdnProbeResult`）。大小未知时，64 KiB 预算已包含用于发现总大小的 1 字节请求；
报告的吞吐量只统计后续样本请求。请只在用户明确触发时调用，因为每次探测都会对 CDN 发起真实请求。

## 下载归档和重复决策

嵌入应用应保持重复处理显式。用 `DownloadPreflight` 检查计划，把已有归档记录或输出冲突
展示给用户，然后用同一份 preflight 和用户选择的 `DuplicateDecision` 调用 executor。crate
不会提示用户。如果应用在展示和执行之间序列化 preflight，请存储完整 preflight 对象，这样
`KeepBoth` 仍会避开检查时保留的 archive-only 输出目录。executor 会在应用决策前校验
preflight 仍匹配当前归档，因此当另一个进程可能更新归档时，调用方应重新检查。
Archive 匹配会区分 single-output mode 和弹幕格式，因此 ASS-only 或 multi-format 弹幕下载
不会满足 XML-only 弹幕 preflight。

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DownloadArchive, DownloadOptions, DownloadPreflight,
    DuplicateDecision, MuxOptions,
};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client.plan_download("BV1qt4y1X7TW", None).await?;
    let options = DownloadOptions::new("downloads").with_mux(MuxOptions::Disabled);
    let archive_path = "downloads/archive.json";
    let mut archive = DownloadArchive::load(archive_path)?;
    let preflight = DownloadPreflight::inspect(&plan, &options, Some(&archive))?;

    if preflight.requires_decision() {
        println!(
            "{} possible duplicate records",
            preflight.archived_records.len()
        );
        if let Some(conflict) = &preflight.output_conflict {
            println!("output exists: {}", conflict.path.display());
        }
    }

    let decision = if preflight.requires_decision() {
        DuplicateDecision::KeepBoth
    } else {
        DuplicateDecision::Cancel
    };
    let report = client
        .download_plan_with_archive_preflight_decision(
            &plan,
            options,
            &mut archive,
            &preflight,
            decision,
        )
        .await?;
    archive.save(archive_path)?;

    println!("wrote {}", report.output_dir.display());
    Ok(())
}
```

`DuplicateDecision::Replace` 会在已有计划输出根目录存在时先删除它，再重新下载；完成后的记
录会替换指向同一输出路径的陈旧归档记录。`DuplicateDecision::KeepBoth` 会写入下一个带后
缀的输出根目录，并保留旧归档记录，包括旧输出目录已被移除的 archive-only 记录。如果 UI
在重复预检查后选择取消，应在 preflight 后停止，不要调用下载 executor。对无冲突 preflight
传入 `DuplicateDecision::Cancel` 是安全的继续路径：如果输出冲突在执行前出现，executor
会报告它，而不是隐式替换新的输出根目录。归档记录包含内容身份、绝对输出路径、条目 id、
绝对旁路文件路径、绝对 mux 输出路径和完成时间戳；不包含媒体 URL 或凭据。条目身份使用
aid/cid 媒体 id，而不是可选 BVID 或分集 id，因此通过分集 URL 规划的 PGC 分集可以匹配之
后同一媒体的 BV/av 计划，即便其中一个计划缺少 BVID。
需要按 mode 精确查询归档时使用 `DownloadArchive::records_for_plan_with_mode`；当 UI 希
望展示同一内容在所有 download mode 下的归档记录时使用 `records_for_plan`。
`DownloadPreflight::inspect` 也会把相同计划输出路径的归档记录视为重复，即便内容身份不同
且旧输出目录已经不在磁盘上。把归档存放在所选输出根目录和任何归档保存旁路路径之外的
JSON 文件路径；`DownloadArchive::save` 会拒绝目录目标。如果归档路径是符号链接，
`DownloadArchive::save` 会更新符号链接目标，而不是替换链接本身。

仅追加式弹幕刷新是针对已下载条目的独立归档操作。用相同输入和 selection 按
`DownloadMode::DanmakuOnly` 生成 plan，加载归档，然后调用
`BiliClient::update_danmaku_for_archive`。仅弹幕模式的规划不依赖媒体 playurl 是否可用。
该方法按 aid/cid 匹配归档条目，下载当前 XML 弹幕数据，将新弹幕追加合并到
`danmaku.xml`，重新生成所选派生格式（例如 ASS），更新归档条目的旁路文件列表，并返回带类型的
`DanmakuUpdateReport`，其中包含每个条目已有、拉取和追加的弹幕数量。XML 始终作为更新的基准文件；
`DanmakuUpdateOptions::with_danmaku_formats([DanmakuFormat::Ass])` 会根据合并后的 XML 新增或刷新
`danmaku.ass`。

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DanmakuFormat, DanmakuUpdateOptions, DownloadArchive, DownloadMode,
};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client
        .plan_download_with_mode("BV1qt4y1X7TW", None, DownloadMode::DanmakuOnly)
        .await?;
    let archive_path = "downloads/archive.json";
    let mut archive = DownloadArchive::load(archive_path)?;
    let report = client
        .update_danmaku_for_archive(
            &plan,
            &mut archive,
            DanmakuUpdateOptions::default().with_danmaku_formats([DanmakuFormat::Ass]),
        )
        .await?;
    archive.save(archive_path)?;

    println!("updated {} entries", report.entries.len());
    Ok(())
}
```

如果调用方自行管理旁路文件存储，可以直接使用 `merge_xml_append_only(existing, fetched)`，
无需访问 `DownloadArchive`，即可复用相同的 XML 级仅追加合并逻辑。

当嵌入应用需要保留 XML 历史，并将归档与旁路文件一起发布时，使用
`DanmakuUpdatePolicy::Preserve` 和 `stage_preserving_danmaku_update_for_archive_file`。暂存
结果提供不可变报告、更新后的归档快照、暂存文件列表，以及各文件的 ASS 生成事件统计。
ASS 会从合并后的 XML 重新生成，不会保留旧 ASS 文档。
每个暂存文件都提供目标路径、目标原先存在时的预期字节，以及完整的新输出字节。调用方可以先检查这些值，
再消费暂存结果并调用 `publish()`：

```rust,no_run
use bbdown_core::{
    BiliClient, ClientConfig, DanmakuFormat, DanmakuUpdateOptions, DanmakuUpdatePolicy,
    DownloadMode,
};

#[tokio::main]
async fn main() -> bbdown_core::Result<()> {
    let client = BiliClient::new(ClientConfig::default());
    let plan = client
        .plan_download_with_mode("BV1qt4y1X7TW", None, DownloadMode::DanmakuOnly)
        .await?;
    let staged = client
        .stage_preserving_danmaku_update_for_archive_file(
            &plan,
            "downloads/archive.json",
            DanmakuUpdateOptions::default()
                .with_danmaku_formats([DanmakuFormat::Xml, DanmakuFormat::Ass])
                .with_update_policy(DanmakuUpdatePolicy::Preserve),
        )
        .await?;
    for file in staged.files() {
        println!("staged {} bytes for {}", file.output_bytes().len(), file.path().display());
    }
    for stats in staged.ass_statistics() {
        println!("ASS generated {} events", stats.generated_events);
    }
    let report = staged.publish()?;
    println!("updated {} entries", report.entries.len());
    Ok(())
}
```

暂存结果会记录预期内容，发布前会重新校验选定的目标。`StagedDanmakuFile::path()` 返回解析
父目录符号链接后的选定目标；发布时会针对该目标重新校验逻辑路径别名。发布准备阶段会捕获已有目标的
Unix 权限位或可移植的只读设置，将其应用到替换文件和恢复副本，并在替换目标前再次校验。
Windows 删除待替换目标时会临时清除其只读属性；删除失败会恢复原属性，回滚时也会将捕获的属性
重新应用到恢复文件。新建输出沿用默认创建权限。调用方需要协调 File Provider 将占位文件物化为
实际文件的过程，以及其他并发写入；校验可以检测变化，但不会锁住外部写入方。检测到发布错误时会尝试
回滚；如果回滚无法完成，错误会指出保留的恢复文件。调用成功不代表旁路文件和归档之间具备崩溃或
断电原子性。`Preserve` 会保留 XML 历史，只追加与历史不匹配的新弹幕。优先使用 `p[7]` 中的正
ASCII 十进制 ID，并统一忽略前导零：ID 相同，即使拉取后的元数据或文本改变也视为匹配；ID 不同，
即使文本相同也会追加。ID 缺失、为零或格式无效时，改用完整 `p` 属性和解码后的文本作为匹配依据。
只有无命名空间或与 `<i>` 根元素同命名空间的 `<d>` 元素参与匹配和 ASS 渲染；已有 XML 中其他
命名空间的元素会作为未知内容保留。追加节点会携带来自拉取文档的必要命名空间声明，因此根元素命名空间
相同但前缀不同或使用默认命名空间时，绑定仍会保留；根元素命名空间不同时，追加会失败。每次都会从
完整合并 XML 重建所选 ASS，旧自定义样式和事件会被丢弃。没有 XML 基线时，旧 ASS 事件不会作为历史，
生成的 ASS 只反映本次拉取的 XML 数据。`ass_statistics()` 会为每个 ASS 输出报告
`generated_events`。

## 端点覆盖

测试、本地 mock 或受控 gateway 部署可使用 endpoint builder。

```rust,no_run
use bbdown_core::{ClientConfig, EndpointConfig};

let endpoints = EndpointConfig::default()
    .with_api_base("http://127.0.0.1:8080")
    .with_pgc_base("http://127.0.0.1:8080")
    .with_comment_base("http://127.0.0.1:8081");

let config = ClientConfig::default().with_endpoints(endpoints);
```

## 兼容性建议

- 使用 `Default`、`new` 和 `with_*` 方法构造配置值，而不是结构体字面量。
- 通过字段访问或 serde 序列化读取输出模型；除非测试确实需要本地 fixture，否则避免在下
  游代码中构造输出结构体。
- 不要把凭据、二维码登录扫码 URL、QR payload 和 credential-health 原始请求细节写入日志
  或 crash report。
- 把受限区域代理主机视为可信基础设施，因为媒体 URL 和 access key 可能经过这些主机。
