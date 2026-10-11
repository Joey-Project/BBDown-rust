# CDN 基准

可选的实时基准比较同一个已解析视频表示的三种下载方式：固定 host 顺序基线、对该 host 发出
四路 Range 请求，以及在有限 host 集合上发出四路 Range 请求。每组使用独立输出目录，关闭续传和
封装，并执行一次普通媒体下载尝试；Range 场景仍可产生分片重试和整文件回退诊断。重复运行时会轮换
组顺序。只有显式运行 ignored 集成测试才会访问公开视频
和 CDN 服务；普通 CI 不依赖外部服务运行这些场景。

运行命令和环境变量见[用户指南中的 `cdn_benchmark` 小节](user-guide.zh-CN.md#复跑可选的实时-cdn-基准)。
限制为每个表示最多 256 MiB、最多重复三次、最多四个候选 host；需要探测大小时最多读取 64 KiB，
探测期限为五秒；每个下载请求超时为 300 秒。覆盖样本解析和全部重复运行的总期限为 45 分钟。
整轮已读取正文预算为 3 GiB，观察到的请求 ID 预算为 16,384，清理宽限期为五秒。超出预算会取消
整轮，但已读取的超限响应块和在途响应块可能造成正文用量超额，取消与清理也可能延长总耗时。这些
限制不是 wire 流量上限。

## 如何读取数据

三组使用同一媒体计划表示，并按文件大小和 SHA-256 校验输出。摘要相同只能证明本次完成的输出文件
相同，不能证明不同 CDN host 提供的是同一份未变化表示；文件大小和起始采样相同只是兼容性检查，
不能证明整个文件的身份一致。

`transfer_bytes_by_host_phase` 是每组的 `{host, phase, bytes}` 数组，按安全 host authority（或
`null`）和 typed phase 汇总应用实际读取并解码的媒体响应正文块。phase 包括 `automatic_probe`、
`shard_probe`、`range_chunk` 和 `whole_file`。`transfer_total_consumed_body_bytes` 是本组总量；
`preflight_consumed_body_bytes` 汇总两种 probe phase，`media_body_consumed_bytes` 汇总 Range chunk
和整文件响应。API 与身份认证响应正文不在统计范围内；HTTP headers 和传输层 framing 也不计入。
这些字段都不是 wire bytes。

若媒体计划未提供大小，donor 大小探测是三组共享的前置设置。每条组记录都会重复展示
`size_probe_transfer_*` 元数据，并设 `size_probe_included_in_group_totals: false`；汇总多条记录时不要
重复累加这些字段。共享探测只计入整轮 3 GiB 正文预算一次。它使用独立请求 ID 范围和
`standalone_probe` phase。

按 host 和 phase 累加 `TransferBytesReceived.bytes_delta`。`RequestFinished.bytes_received` 是该请求
最终累计水位，不是另一个需要相加的增量。`CdnShardCompleted.bytes` 则统计成功发布的分片字节数，并按
来源 host 汇总。失败、重试、探测或取消的响应正文可能已经读取，却没有发布到文件。typed
`transfer_diagnostics` 会报告候选选择或排除、重试、整文件回退和请求结果。`RequestFinished` 结果计数
与观察到的请求 ID 分开报告。

`observed_request_ids` 中的唯一 ID 来自正文或诊断事件。由于没有请求开始事件，没有发出这两类事件的
请求无法计数，因此 `observed_request_count_lower_bound` 只是下界，不是完整请求数。请求 ID 在每组内
重新编号；共享 donor 探测使用独立范围，并记录在单独的 `size_probe_*` 字段中。

`transfer_activity_by_host_phase` 和 `size_probe_transfer_activity_by_host_phase` 报告每个 host/phase
的首末观测时间及其跨度。诊断项的 `observed_at_elapsed_ms` 使用同一 collector 起始时刻作为参考。这些
时间只覆盖正文或带 phase 的诊断事件；它们不含建连和等待响应头的时间，因此不是完整请求时延，也不能
把并发 host 的时间跨度相加。

应跨样本、host、文件大小和运行时段比较耗时与字节分布。现有观测只有两种表示大小、两个较短的上午
时段和 18 次成功下载；它们不能确立稳定提速、全天或多地点性能，也不足以设置全局默认路由。未来若要
自动选择 CDN，应先收集更广的分布，再考虑不含 URL 或凭据的最少 host 健康状态，以及有界的自适应分片
大小和并发度。必须保留严格 Range 校验、取消、回退和输出目标安全。公共播放地址解析服务器与媒体 CDN
host 选择是不同的显式选项；受限区域 PGC 解析也单独启用。

## 验证记录

`crates/bbdown/tests/support/cdn_benchmark_scenarios.rs` 中的确定性本地场景覆盖小型和大型表示、快速和
延迟 Range 响应、候选排除、在另一 host 上重试部分响应正文、整文件回退，以及预算取消；这些场景不访问
公网。验证状态和 gate 结果见[工作流 journal](project_journal/2026/10/2026-10-11-cdn-performance-baseline-53887b35.md)。
本地 mock 结果不能说明实时性能。
