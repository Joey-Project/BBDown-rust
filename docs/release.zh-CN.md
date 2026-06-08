[[English](release.md) | 简体中文]

# 发布 Runbook

本项目使用两阶段发布：

1. 先手动批准一个 GitHub Actions 运行，对 `master` 做验证、构建 release artifacts，并创建
   `v0.1.0-rc.1` 这样的 release candidate tag。
2. 再从该 RC tag 手动启动第二个受保护 workflow。它会重新构建正式 artifacts、创建
   `v0.1.0` 这样的正式 tag、发布 GitHub Release，并把 `bbdown-core` crate 发布到
   crates.io。

可复用的 release artifact workflow 仍然可以手动运行，用于预览归档；它不会发布 tag、
GitHub Release 或 crate。

## GitHub 设置

配置这些 environments：

- `release-candidate`：只允许从 `master` branch 部署。把 `RELEASE_GITHUB_APP_ID` 和
  `RELEASE_GITHUB_APP_PRIVATE_KEY` 放在这里，或让等价的 repository secrets 对这个
  environment 可用。
- `production-release`：只允许从匹配 `v*-rc.*` 的 tag 部署。这里也放同一组 release
  GitHub App secrets。
- `crates-io`：只允许从匹配 `v*-rc.*` 的 tag 部署。这里放 `CARGO_REGISTRY_TOKEN`。

release GitHub App 需要 repository metadata read 和 contents write 权限。release tag
rulesets 应只允许这个 App 作为非人工 actor 写 tag。

推荐 rulesets：

- `master` branch：要求 pull request，要求 `Rust` 和 `codex/review-gate` status checks，
  要求 Code Owner review。如果普通 PR 不需要人工审批，可以把通用 required approval count
  保持为 `0`。
- RC tags：target `v*-rc.*`；限制创建、更新和删除；只允许 release GitHub App 创建。
- 正式 release tags：target `v*`，如果 UI 支持 exclude，则排除 `v*-rc.*`；否则使用不会匹配
  RC tag 的单独正式 tag pattern。限制创建、更新和删除；只允许 release GitHub App 创建。

## 创建 RC Tag

1. 确认 `crates/bbdown/Cargo.toml` 已经是最终 crate version，例如 `0.1.0`。
2. 确认要发布的分支已经合入 `master`。
3. 在 GitHub Actions 里，从 `master` branch 运行 `Create Release Candidate`。
4. 输入不带前导 `v` 的 `version`，例如 `0.1.0`。workflow 会自动选择下一个可用 RC 编号。
5. 批准 `release-candidate` environment deployment。

该 workflow 会检查它是否从 `master` 运行，验证 `bbdown-core` 和 `bbdown-cli` Cargo
version，按 release version 串行化 RC 创建，计算下一个 RC 编号，运行 formatter、clippy、测
试和 crates.io dry run，构建所有 release archives，然后创建 annotated RC tag。

## 晋升 RC

1. 在 GitHub Actions 里打开 `Promote Release Candidate`。
2. 使用 branch/tag selector 选择 RC tag，例如 `v0.1.0-rc.1`。
3. 启动 workflow。
4. 批准 `production-release` environment deployment。
5. 批准 `crates-io` environment deployment。

该 workflow 会验证选中的 ref 是 RC tag，按 RC tag 串行化 promotion，确认 `bbdown-core` 和
`bbdown-cli` Cargo version 与正式 tag 匹配，重新运行 formatter、clippy、测试和 crates.io
dry run，重新构建正式 release archives，创建正式 annotated tag，发布 GitHub Release，然后
发布 `bbdown-core`。

## 失败恢复

- 如果 RC 创建在 tag 创建前失败，修复问题后用相同 version 和 RC 编号重跑 RC workflow。
- 如果 RC 创建失败时 tag 已经存在，除非 maintainer 有意删除已有 tag，否则使用新的 RC 编号。
- 如果 promotion 在 `Publish GitHub Release` 前失败，修复后从同一个 RC tag 重跑。如果正式
  tag 已经创建且仍指向该 RC target commit，workflow 会复用该 tag 并继续创建缺失的 release。
- 如果 GitHub Release 已经发布成功，但 crates.io 发布失败，使用 GitHub Actions 的
  `Re-run failed jobs`，只重试失败的 crate job。重跑整个 workflow 会失败，因为正式 tag 和
  GitHub Release 已经存在。
- 正式 release 被设计为不覆盖。正式 tag 只有在已经指向同一个 RC target commit 且 GitHub
  Release 仍缺失时才会被复用。要替换有问题的正式 release，请发布新版本。

发布后验证：

```bash
gh release view v0.1.0 --repo Joey-Project/BBDown-rust
cargo search bbdown-core --limit 5
```
