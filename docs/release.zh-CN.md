[[English](release.md) | 简体中文]

# 发布 Runbook

本项目使用两阶段发布：

1. 先手动批准一个 GitHub Actions 运行，对 repository default branch 做验证、构建 release
   artifacts，并创建 `v0.1.0-rc.1` 这样的 release candidate tag。
2. 再从该 RC tag 手动启动第二个受保护 workflow。它会重新构建正式 artifacts、创建
   `v0.1.0` 这样的正式 tag、发布 GitHub Release，并把 `bbdown-core` crate 发布到
   crates.io。

可复用的 release artifact workflow 仍然可以手动运行，用于预览归档；它不会发布 tag、
GitHub Release 或 crate。

## GitHub 设置

配置这些 environments：

- `release-candidate`：只允许从 repository default branch 部署，目前是 `master`。把
  `RELEASE_GITHUB_APP_ID` 和 `RELEASE_GITHUB_APP_PRIVATE_KEY` 放在这里，或让等价的
  repository secrets 对这个 environment 可用。
- `production-release`：只允许从匹配 `v*-rc.*` 的 tag 部署。这里也放同一组 release
  GitHub App secrets。
- `crates-io`：只允许从匹配 `v*-rc.*` 的 tag 部署。这里放 `CARGO_REGISTRY_TOKEN`。

release GitHub App 需要 repository metadata read 和 contents write 权限。release tag
rulesets 应只允许这个 App 作为非人工 actor 写 tag。

推荐 rulesets：

- Repository default branch，目前是 `master`：要求 pull request，要求 `Rust` 和
  `codex/review-gate` status checks，要求 Code Owner review。如果普通 PR 不需要人工审
  批，可以把通用 required approval count 保持为 `0`。
- RC tags：target `v*-rc.*`；限制创建、更新和删除；只允许 release GitHub App 创建。
- 正式 release tags：target `v*`，如果 UI 支持 exclude，则排除 `v*-rc.*`；否则使用不会匹配
  RC tag 的单独正式 tag pattern。限制创建、更新和删除；只允许 release GitHub App 创建。

## 创建 RC Tag

1. 确认 `crates/bbdown/Cargo.toml` 已经是最终 crate version，例如 `0.1.0`。
2. 确认要发布的分支已经合入 repository default branch，目前是 `master`。
3. 在 GitHub Actions 里，从 repository default branch 运行 `Create Release Candidate`。
4. 输入不带前导 `v` 的 `version`，例如 `0.1.0`。workflow 会自动选择下一个可用 RC 编号。
5. 批准 `release-candidate` environment deployment。

该 workflow 会检查它是否从 repository default branch 运行，验证 `bbdown-core` 和
`bbdown-cli` Cargo version，按同一 release version 串行化所有 RC 创建和 promotion 运行，
计算下一个 RC 编号，运行 formatter、clippy、declared MSRV check、测试和 crates.io dry
run，构建所有 release archives，并拒绝已经存在 final tag 或 GitHub Release 的 version，
然后创建 annotated RC tag；真正写入前还会再次检查 final tag 和 GitHub Release 状态。

## 晋升 RC

1. 在 GitHub Actions 里打开 `Promote Release Candidate`。
2. 使用 branch/tag selector 选择该版本最新的 RC tag，例如 `v0.1.0-rc.1`。
3. 输入不带前导 `v` 的最终 SemVer `version`，例如 `0.1.0`。它必须与所选 RC tag 匹配，并
   用于按最终 release version 串行化 promotion。
4. 启动 workflow。
5. 批准 `production-release` environment deployment。
6. 批准 `crates-io` environment deployment。

该 workflow 会验证选中的 ref 是请求版本的最新 RC tag，按最终 release version 串行化
promotion，确认 `bbdown-core` 和 `bbdown-cli` Cargo version 与正式 tag 匹配，重新运行
formatter、clippy、declared MSRV check、测试和 crates.io dry run，重新构建正式 release
archives，并在发布前再次确认选中的 RC 仍然是最新 RC，然后创建正式 annotated tag，发布
GitHub Release，并发布 `bbdown-core`。如果存在上一条非 RC 正式 release tag，自动生成的
GitHub Release notes 会从那条 tag 开始，避免把刚创建的 RC tag 当作比较起点。如果
crates.io 已经存在 exact `bbdown-core` version，crate publish step 会把它当作恢复成功。

## 失败恢复

- 如果 RC 创建在 tag 创建前失败，修复问题后用相同 version 重跑 RC workflow。
- 如果 RC 创建失败时 tag 已经存在，用相同 version 重跑 workflow；它会自动选择下一个 RC
  编号，除非 maintainer 有意删除已有 tag。
- 如果 final tag 或 GitHub Release 已经存在，不要再为该 version 创建 RC；改发新 version。
- 如果 promotion 在 `Publish GitHub Release` 前失败，修复后从同一个 RC tag 重跑。如果正式
  tag 已经创建且仍指向该 RC target commit，workflow 会复用该 tag。
- 如果 promotion 留下了 draft GitHub Release，从同一个 RC tag 重跑即可。workflow 会删除该
  draft，并用重新构建的 assets 重新创建 release。
- 如果 GitHub Release 已经发布成功，但 crates.io 发布失败，优先使用 GitHub Actions 的
  `Re-run failed jobs`，只重试失败的 crate job，避免重新构建 artifacts。从同一个 RC tag 重
  跑整个 workflow 是 fail-closed 的：workflow 只会在已发布 GitHub Release 的 asset 名称与
  预期名称完全一致、每个 asset 都是 `uploaded` 且非空，并且下载后的归档能通过其已发布
  `.sha256` sidecar 校验时复用它，然后继续发布到 crates.io。Release archives 会规范化条目
  顺序、时间戳、owner、group 和归档容器 metadata，因此相同已编译输入会得到稳定 checksum；
  但已发布 release 复用校验的是已经发布的 assets，而不是要求重新构建出的二进制字节一致。
  如果 exact crate version 已经被 crates.io 接受，crate publish step 会成功退出。
- 正式 release 被设计为不覆盖。正式 tag 只有在已经指向同一个 RC target commit 时才会被复
  用；已发布 GitHub Release 只有在 asset set 完整且 checksum 校验通过时才会被复用。要替
  换有问题的正式 release，请发布新版本。

发布后验证：

```bash
gh release view v0.1.0 --repo Joey-Project/BBDown-rust
cargo search bbdown-core --limit 5
```
