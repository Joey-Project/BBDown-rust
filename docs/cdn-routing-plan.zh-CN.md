# 自动 CDN 选路计划

本文是供后续独立实现 PR 评审的设计文件，本文件不报告测量结果，也不改变当前选路。历史基线覆盖两种表示大小、两个较短的上午时段和 18 次成功下载。后续两次有效的三组运行记录在[扩展 live 测量 journal](project_journal/2026/10/2026-10-11-cdn-expanded-live-measurement-01a12c87.md)。这些结果都不能证明稳定提速、全天或多地点性能，也不足以确定全局默认路线。传输 activity 时间戳不含建连和等待响应头时间，因此不能称为完整请求时延或 TTFB。

## 现有约定与显式启用边界

- 拟议自动策略关闭时（默认），保留当前有序媒体 URL 行为。`DownloadOptions::default()` 中 `cdn_probe = false`、`cdn_parallelism = 1`；`MediaHostOptions` 不配置替换 host 或显式候选池。不得静默增加探测、host 改写或持久化路线状态。
- 现有 `--cdn-probe` 和 `--cdn-parallel N` 已会在新目标路径触发 downloader 的自动候选探测。`should_probe_media_candidates` 的条件为 `cdn_probe || cdn_parallelism > 1`，但非普通文件目标、续传且目标已有内容时除外。媒体流和 FLV 分段都走此路径。`probe_media_candidates_with_progress` 最多探测现有 primary/backup/配置 donor URL 中的前八个，校验 Range 响应，按总长度和匹配前缀对成功样本分组，按实测吞吐降序提升最大匹配样本组；失败或其他 URL 保留原顺序作为回退。若没有可用测量，则保留原 URL 顺序和预期大小。因此 `--cdn-probe` 当前确实会对兼容且已测量的一组候选重新排序，`--cdn-parallel` 也会在独立的分片传输尝试前触发同一排序。
- `--cdn-auto` 是提议中的独立 opt-in 策略层，不取代或重新定义这些旧 flags。它必须保留它们现有的触发与回退行为。auto 模式先构建并限制最终候选 URL 列表，再进入现有唯一一次 automatic-probe 流程；旧 `--cdn-probe` / `--cdn-parallel` 与 auto 共用这次探测和排序，绝不对同一 auto 列表重复探测。若旧 flags 均未触发探测，则 auto 触发同一流程一次。关闭 auto 时，旧 flags 的候选列表和行为完全维持现状。后续 shard 兼容探测是不同操作，分片传输仍需要它。
- 固定手动控制的组合规则：`--upos-host` 是单 host 强制替换，与 `--cdn-auto` 冲突（组合时拒绝）。显式 `--cdn-host` 列表或 `--cdn-preset` 是允许的候选池；auto 只在该池与原始 donor 中排序，配置顺序作确定性 tie-break 和回退。没有用户候选池时，显式 opt-in 策略使用内置的 overseas 媒体候选集和原始 donor；不推断用户位置，也不凭空生成 host。保留现有 `--cdn-host` 与 `--cdn-preset` CLI 互斥规则。
- `ClientConfig` 的 endpoint、凭据、playurl 模式和 PGC 路由与媒体 host 策略分开。`bbdown-core` 调用方通过下载选项显式启用；CLI 只从明确的下载 flag/config 映射该策略。公共 playurl resolver 选择及受限区域 PGC 路由不得参与媒体 host 排名。Resolver/API 错误归属于 resolver/API 操作，不能记入某个 CDN host 的健康状态。
- 自动模式只允许改写媒体 donor URL 的 authority。必须原样保留 scheme、签名 path、query 和所需媒体 headers；不得记录、存储或发出凭据、cookie、完整 URL、path 或 query。诊断只显示清理后的 host authority 和稳定的原因枚举。

## 候选选择与安全

1. 未来 auto 模式最多构建四个候选 URL，每个规范化 authority `(scheme, host, effective port)` 仅一个。先保留 primary resolved URL 作为原始 donor。然后按顺序遍历选中的允许候选池（用户配置的 `--cdn-host` / `--cdn-preset`；若没有用户池，则用内置 overseas pool）：规范化 authority，跳过无效项、donor authority 和重复项，最多取剩余前三个。每个入选 authority 若在同一主/backup URL 列表中已有对应地址且该 authority 获准，则按 primary 优先、其次 backup 顺序选第一个；否则将 primary donor URL 改写为该 authority。达到三个 pool authority 后停止，后续候选忽略。backup URL 只有在其规范化 authority 已属于所选允许池时才可提供候选；backup 不能扩展候选池或产生第五个 URL。原始 donor 保留作回退。不能混合画质、流 ID 或分别解析的 playurl 响应。auto 关闭时，现有完整 primary/backup/manual 候选构建行为保持不变。
2. Stage 1 是未来 PR 的提案。复用 downloader 现有的自动候选采样过程，不复用独立公开的 `probe_media_cdns` 命令/API，也不重复测量。现有 downloader 探测最多处理八个已有候选，每候选最多读取 64 KiB；当前单请求 timeout 为两秒（正文 idle timeout 为一秒）。拟议 auto 层可以设置五秒的候选测量总上限，但复用已有结果时不得延长或重复请求。拟议 auto 候选池最多四个不同 authority（含原始 donor），名义采样正文预算最多为 256 KiB，另需考虑在途请求/取消造成的超额。实时基准的 donor-size setup probe 使用独立的 64 KiB/五秒上限。内部 shard 兼容探测也保持独立：每候选最多采样 16 KiB，单独受两秒上限约束，用于校验 Range 源兼容性，不用于吞吐排名。固定累计预算前，先在 mock 中核对实际正文用量和取消超额。
3. 候选必须通过精确 HTTP 206、与请求一致的 `Content-Range`、总长度、存在时的声明 `Content-Length` 和实际正文长度校验才可入选。候选分组也可要求 scheme、path/query、总长度和采样正文相同。采样和长度匹配只代表“本次尝试兼容”，不代表完整文件身份一致。不能仅凭这些证据混合不同表示。
4. 当前下载前排序已经是实测样本排序：成功 probe 中，只有总长度与最大匹配前缀组都相同的候选参与；按采样正文长度除以完整候选探测耗时，按吞吐降序排序；并列时保留输入顺序。拟议 Stage 1 新增的是上文有界 opt-in 候选池策略，不重复吞吐排序或测量。auto 先形成最终候选列表，再进入唯一一次现有探测流程并使用该流程排序结果；失败和不兼容样本保留原顺序回退。传输 activity 事件不含 request-start 和响应头等待，因此不是完整 latency/TTFB；不能从 activity 跨度推导排序时延，也不能累加并发 host 的跨度。
5. 单 host 并行 Range 下载必须继续有效：若只有一个兼容候选，所有 lane 都可以使用它，符合当前行为。成功分片不要求 host 多样性。旧 flags 触发时，即使只有一个 URL，当前 probe 仍可能运行，但排序一个 authority 没有选路效果。未来 auto 候选列表若只剩原始 donor，也不增加 host 改写或重排。
6. Range 失败时可在其他兼容候选上重试完全相同的字节区间。所有正文先写入临时存储，仅当每个区间都通过校验后才发布。普通可恢复的分片错误使用现有整文件路径并报告原因。取消必须终止：向上传播取消，不得再启动整文件回退或其他路线请求。目标校验与原子发布仍是必需条件。

签名 URL 需要特别谨慎：即使保留签名 path/query，也不能证明其他 authority 接受该签名、映射到同一对象或提供稳定表示。候选专属的传输、HTTP 或 Range 不匹配只在本次尝试中排除该候选。过期/无效签名等可能影响整个已签名表示的响应，不能让所有 host 都被全局判坏；应保留普通下载错误/回退行为，并将 resolver/playurl 错误归到对应操作。CDN 选路不得顺带刷新凭据或调用其他 resolver。

## Host 健康状态

- Stage 2 加入 host 健康状态时，首版只使用进程内状态。键为规范化 scheme、host 和有效端口；绝不使用 URL、path、query、媒体 ID、用户 ID、凭据或 cookie 作为键。只保存排序需要的有界计数/结果及 probe 时长、吞吐摘要，不保存媒体采样正文。
- Stage 2 建议的实验边界：最多保存 64 个 host 记录，TTL 为 15 分钟，每个 host 仅保留少量近期样本（例如最近 8 次结果）。记录到 TTL 后过期。成功 probe 会清除该 host 的失败惩罚，并在原 host 记录上新增或刷新成功测量；不会删除 host 记录。用户关闭选路、缓存版本变化或显式重置时清空全部状态。不缓存凭据或签名 URL 有效性。首版进程退出后自然清空状态。
- 如果后续证据支持磁盘持久化，应另设显式开关，使用应用 cache 目录下带版本的 host-only 缓存、受限权限、原子替换、记录/文件大小上限和相同 TTL。缓存缺失、损坏、不可读或版本过旧时回退当前有序路线，不能导致下载失败。没有证据证明需要前，不在首个选路 PR 加入持久状态。
- 只统计可归因于 host 的 probe/传输结果。playurl/API 解析失败、缺少凭据、取消、本地磁盘错误、目标拒绝或归因不明的签名过期，都不能让所有候选变成不健康。要区分未知/不可读状态与已过期记录。

## 分阶段交付与验收

| 阶段 | 范围与 mock CI 验收 | 显式启用的实时验收与回滚 |
| --- | --- | --- |
| 0. 证据 | 用确定性本地场景覆盖候选预算、精确 Range/正文校验、host 顺序和并列排序、单 host 并发、URL 脱敏、取消及可恢复回退。断言默认选项不增加探测且保持现有 URL 顺序。 | 不作性能声明。关闭功能即可回到现有行为。 |
| 1. 显式选路 | 增加显式 auto 候选池及其与现有探测流程的组合；auto 默认关闭，所有旧 flag 行为保持不变。Mock 覆盖内置候选池、手动候选池、`upos_host` 冲突、四 URL/authority 上限、确定性去重与超额截断、backup 候选资格、同列表结果复用、不重复 automatic probe、单 authority 无操作、样本排序，以及探测失败/不可用时的普通回退。 | 对同一已解析表示、独立输出目录，比较固定 host 顺序下载、固定 host Range、多 host Range；轮换顺序并跨大小和时段重复。记录输出、hash、实际发布 host、已读取正文、回退与错误。关闭 `--cdn-auto` 即回滚。 |
| 2. Host 健康状态 | 加入有界进程内状态、TTL/过期、重置，并确定性测试成功/失败更新、隔离、过期及无敏感字段。成功 probe 清除失败惩罚并刷新既有 host 记录上的样本。 | 按相同表示和对照协议比较健康度调整后的排序、Stage 1 采样排序与固定配置顺序。健康度调整保持 opt-in；丢弃状态或关闭功能即可回滚。 |
| 3. 传输自适应 | 单独评估有界 chunk 大小/并发变化。到这一阶段前保留显式配置并发度；每次比较只改变一个变量，并保留单 host 行为。 | 要求相同表示的重复对照、多种大小和时段、完整 hash/长度、host 字节合计、已读正文统计、重试/回退和取消检查。回归或证据不确定时恢复固定 chunk/并发设置。 |

实时运行始终显式启用。本文是供后续独立 PR 评审的提案，本设计本身不报告测量；项目实时结果见[扩展 live 测量 journal](project_journal/2026/10/2026-10-11-cdn-expanded-live-measurement-01a12c87.md)。单个有效样本不能保证稳定提速，也不足以选择默认 host、健康度权重、TTL、chunk 大小或并发度。上文的提议值只是可测试的起始边界，应依据测量更新。公共 resolver 和 PGC 路由实验与媒体传输矩阵分开。

## 接口与诊断后续

当前已有 `MediaHostOptions`、`DownloadOptions::with_cdn_probe`、`with_cdn_parallelism`、内部 `probe_media_candidates_with_progress` 排序过程、`probe_media_cdns`、Range 诊断，以及 CLI host/preset/probe/并发开关。后续 PR 应新增独立 opt-in 候选池策略，不改变旧选项语义。诊断只在需要时增加脱敏 host、phase、有界结果和操作实测耗时。要提供完整请求 latency/TTFB，先补 request-start 与响应头计时；`transfer_activity_by_host_phase` 仍只代表正文/诊断 activity。已解码正文用量须与 wire bytes 区分；`RequestFinished.bytes_received` 继续作为最终水位值，不能重复当作增量相加。
