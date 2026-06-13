[[English](release.md) | 简体中文]

# 发布 Runbook

本项目使用两阶段发布：

1. 先手动批准一个 GitHub Actions 运行；它从要发布的 source branch 启动，验证该 source、
   构建 release artifacts，并创建 `v0.1.0-rc.1` 这样的 release candidate tag。
2. 再从该 RC tag 手动启动第二个受保护 workflow。它会重新构建正式 artifacts、创建
   `v0.1.0` 这样的正式 tag、发布 GitHub Release，并把 `bbdown-core` crate 发布到
   crates.io。

可复用的 release artifact workflow 仍然可以手动运行，用于预览归档；它不会发布 tag、
GitHub Release 或 crate。
RC 创建和 RC promotion 共用 `Release Verification` reusable workflow 来执行 formatter、
lint、declared MSRV、测试和 crates.io dry-run validation。

## GitHub 设置

配置这些 environments：

- `release-candidate`：允许从 repository default branch，目前是 `master`，以及匹配
  `release/*` 的 release branches 部署。把 `RELEASE_APP_CLIENT_ID` 作为 environment
  variable 放在这里，它的值是传给
  `actions/create-github-app-token` 的 release GitHub App ID；把 `RELEASE_APP_PRIVATE_KEY`
  作为 environment secret 放在这里；也可以让等价的 repository-level 配置对这个 environment
  可用。
- `production-release`：只允许从匹配 `v*-rc.*` 的 tag 部署。这里也放同一组 release
  GitHub App variable 和 secret。
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

1. 确认 `crates/bbdown/Cargo.toml` 和 `crates/bbdown-cli/Cargo.toml` 都已经是最终版本，
   例如 `0.1.0`。如果当前开发线包含破坏性的公开 crate API 变更，请选择下一个 pre-`1.0`
   breaking 版本，例如已发布 `0.2.x` 线之后使用 `0.3.0`。
2. 确认要发布的 source 要么已经合入 repository default branch，目前是 `master`，要么已经
   作为 release branch 存在，例如 `release/0.2.0`。
3. 在 GitHub Actions 里，从被发布的分支运行 `Create Release Candidate`。当前开发线版本从
   `master` 运行；维护版本从对应的 `release/*` 分支运行。
4. 输入不带前导 `v` 的 `version`，例如 `0.1.0`。workflow 会自动选择下一个可用 RC 编号。
5. 批准 `release-candidate` environment deployment。

该 workflow 会检查它是否从 repository default branch 或 `release/*` 分支运行，验证
`bbdown-core` 和 `bbdown-cli` 在该分支上的 Cargo version，按同一 release version 串行化所有 RC
创建和 promotion 运行，计算下一个 RC 编号，调用共享 release verification workflow，构建所
有 release archives，并拒绝已经存在 final tag 或 GitHub Release 的 version，然后创建
指向 workflow ref commit 的 annotated RC tag；真正写入前还会再次检查 final tag 和 GitHub
Release 状态。

## 晋升 RC

1. 在 GitHub Actions 里打开 `Promote Release Candidate`。
2. 使用 branch/tag selector 选择该版本最新的 RC tag，例如 `v0.1.0-rc.1`。
3. 输入不带前导 `v` 的最终 SemVer `version`，例如 `0.1.0`。它必须与所选 RC tag 匹配，并
   用于按最终 release version 串行化 promotion。
4. 启动 workflow。
5. 批准 `production-release` environment deployment。
6. 批准 `crates-io` environment deployment。

该 workflow 会验证选中的 ref 是请求版本的最新 RC tag，按最终 release version 串行化
promotion，确认 `bbdown-core` 和 `bbdown-cli` Cargo version 与正式 tag 匹配，调用共享
release verification workflow，重新构建正式 release archives，并在发布前再次确认选中的
RC 仍然是最新 RC，在创建正式 tag 或 GitHub Release 前验证 crates.io 上任何已存在的
`bbdown-core` version 都没有被 yank 且与本地 package checksum 匹配，然后创建正式
annotated tag，发布 GitHub Release，并发布 `bbdown-core`。如果存在上一条非 RC 正式
release tag，自动生成的
GitHub Release notes 会从那条 tag 开始，避免把刚创建的 RC tag 当作比较起点。如果
crates.io 已经存在 exact `bbdown-core` version，crate publish step 会重新打包选中的 RC
源码，并且只在本地 `.crate` SHA256 与 crates.io checksum 匹配时把它当作恢复成功。

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
  跑整个 workflow 是 fail-closed 的：workflow 只会在已发布 GitHub Release 没有被标记为
  prerelease、asset 名称与预期名称完全一致、每个 asset 都是 `uploaded` 且非空，并且下载后
  的归档能通过其已发布 `.sha256` sidecar 点名并校验通过，且重新构建出的 `dist` 归档也能
  通过自己的 sidecar 校验、archive checksum 与已发布 assets 相同的时候复用它，然后继续发
  布到 crates.io。Release archives 会规范化条目顺序、时间戳、owner、group 和归档容器
  metadata，因此相同已编译输入会得到稳定 package checksum。如果 exact crate version 已经
  被 crates.io 接受，crate publish step 只有在已存在版本没有被 yank、且当前 RC package
  checksum 与 crates.io checksum 匹配后才会成功退出。
- 正式 release 被设计为不覆盖。正式 tag 只有在已经指向同一个 RC target commit 时才会被复
  用；已发布 GitHub Release 只有在它不是 prerelease、且 asset set 完整并通过 checksum 校验
  时才会被复用。要替换有问题的正式 release，请发布新版本。

发布后验证：

```bash
gh release view v0.1.0 --repo Joey-Project/BBDown-rust
cargo search bbdown-core --limit 5
```
