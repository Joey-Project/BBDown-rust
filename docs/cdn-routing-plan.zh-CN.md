# 自动 CDN 选路计划

本文是供后续独立实现 PR 评审的设计文件，本文件不报告测量结果，也不改变当前选路。历史基线覆盖两种表示大小、两个较短的上午时段和 18 次成功下载。后续两次有效的三组运行记录在[扩展 live 测量 journal](project_journal/2026/10/2026-10-11-cdn-expanded-live-measurement-01a12c87.md)。这些结果都不能证明稳定提速、全天或多地点性能，也不足以确定全局默认路线。传输 activity 时间戳不含建连和等待响应头时间，因此不能称为完整请求时延或 TTFB。

## 现有约定与显式启用边界

- 自动选择关闭时（默认），保留当前有序媒体 URL 行为。`DownloadOptions::default()` 中 `cdn_probe = false`、`cdn_parallelism = 1`；`MediaHostOptions` 不配置替换 host 或候选池。不得静默增加探测、host 改写或持久化路线状态。
- 手动 `upos_host`、有序 `cdn_hosts`、`cdn_preset`、`cdn_probe` 和 `cdn_parallel` 继续按现有含义生效。自动选路使用独立的显式开关（`--cdn-auto` 及对应的 `DownloadOptions` builder/config 字段）；preset、`--cdn-probe` 或并发度都不能隐式启用它。冲突时应明确报错或定义组合规则，不能静默混用策略。
- `ClientConfig` 的 endpoint、凭据、playurl 模式和 PGC 路由与媒体 host 策略分开。`bbdown-core` 调用方通过下载选项显式启用；CLI 只从明确的下载 flag/config 映射该策略。公共 playurl resolver 选择及受限区域 PGC 路由不得参与媒体 host 排名。Resolver/API 错误归属于 resolver/API 操作，不能记入某个 CDN host 的健康状态。
- 自动模式只允许改写媒体 donor URL 的 authority。必须原样保留 scheme、签名 path、query 和所需媒体 headers；不得记录、存储或发出凭据、cookie、完整 URL、path 或 query。诊断只显示清理后的 host authority 和稳定的原因枚举。

## 候选选择与安全

1. 从同一个已解析表示的主 URL、备份 URL 和显式配置的媒体 host 候选构建列表。不能混合画质、流 ID 或分别解析的 playurl 响应。原始 donor URL 保留为回退路线。
2. Stage 1 自动选路复用现有 `probe_media_cdns` 完整采样操作：每个候选最多读取 64 KiB 响应正文，探测期限五秒，包含大小未知时的发现请求。初期实验将每次选择限制为最多四个不同 authority（含原始 host），因此名义采样正文总预算最多为 256 KiB，另需考虑在途请求/取消造成的超额。这些是显式启用时的预算，不是测得的最优值。它与内部 shard compatibility probe 分开：后者每个候选最多采样 16 KiB，独立使用两秒期限，用于校验 Range 兼容性，不是 64 KiB 排名测量。固定累计预算前，先在 mock 中核对实际正文用量和取消超额。
3. 候选必须通过精确 HTTP 206、与请求一致的 `Content-Range`、总长度、存在时的声明 `Content-Length` 和实际正文长度校验才可入选。候选分组也可要求 scheme、path/query、总长度和采样正文相同。采样和长度匹配只代表“本次尝试兼容”，不代表完整文件身份一致。不能仅凭这些证据混合不同表示。
4. Stage 1 尚无 host 健康状态。只对 `probe_media_cdns` 完整采样成功且通过响应/Range/长度校验的候选参与排序：按实测样本吞吐从高到低，再按该完整 probe 操作耗时从短到长，最后按配置候选顺序作确定性 tie-break。吞吐用采样正文长度除以完整采样操作耗时。失败或没有测量结果的候选不参与排名；如果没有可用测量，则保留普通有序/原始 URL 回退。这是后续显式启用验证的采样排序提案，不代表全局最佳 host。传输 activity 事件不含 request-start 和响应头等待，因此不是完整 latency/TTFB；不能从 activity 跨度推导排序时延，也不能累加并发 host 的跨度。
5. 单 host 并行 Range 下载必须继续有效：若只有一个兼容候选，所有 lane 都可以使用它，符合当前行为。成功分片不要求 host 多样性。
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
| 1. 显式选路 | 只增加显式 opt-in 策略和上述实测采样排序；静态路线仍为默认。Mock 覆盖原始 host 成功、候选失败、同 host 回退、签名 path/query 保留、resolver 错误隔离、测量并列，以及所有 probe 失败/不可用时的普通回退。 | 对同一已解析表示、独立输出目录，比较固定 host 顺序下载、固定 host Range、多 host Range；轮换顺序并跨大小和时段重复。记录输出、hash、实际发布 host、已读取正文、回退与错误。关闭 `--cdn-auto` 即回滚。 |
| 2. Host 健康状态 | 加入有界进程内状态、TTL/过期、重置，并确定性测试成功/失败更新、隔离、过期及无敏感字段。成功 probe 清除失败惩罚并刷新既有 host 记录上的样本。 | 按相同表示和对照协议比较健康度调整后的排序、Stage 1 采样排序与固定配置顺序。健康度调整保持 opt-in；丢弃状态或关闭功能即可回滚。 |
| 3. 传输自适应 | 单独评估有界 chunk 大小/并发变化。到这一阶段前保留显式配置并发度；每次比较只改变一个变量，并保留单 host 行为。 | 要求相同表示的重复对照、多种大小和时段、完整 hash/长度、host 字节合计、已读正文统计、重试/回退和取消检查。回归或证据不确定时恢复固定 chunk/并发设置。 |

实时运行始终显式启用。本文是供后续独立 PR 评审的提案，本设计本身不报告测量；项目实时结果见[扩展 live 测量 journal](project_journal/2026/10/2026-10-11-cdn-expanded-live-measurement-01a12c87.md)。单个有效样本不能保证稳定提速，也不足以选择默认 host、健康度权重、TTL、chunk 大小或并发度。上文的提议值只是可测试的起始边界，应依据测量更新。公共 resolver 和 PGC 路由实验与媒体传输矩阵分开。

## 接口与诊断后续

当前已有 `MediaHostOptions`、`DownloadOptions::with_cdn_probe`、`with_cdn_parallelism`、`probe_media_cdns`、Range 诊断，以及 CLI host/preset/probe/并发开关。后续 PR 应新增独立 opt-in 策略，不改变这些选项的现有语义。诊断只在需要时增加脱敏 host、phase、有界结果和操作实测耗时。要提供完整请求 latency/TTFB，先补 request-start 与响应头计时；`transfer_activity_by_host_phase` 仍只代表正文/诊断 activity。已解码正文用量须与 wire bytes 区分；`RequestFinished.bytes_received` 继续作为最终水位值，不能重复当作增量相加。
