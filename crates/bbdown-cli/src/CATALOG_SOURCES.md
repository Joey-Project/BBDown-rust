# Bundled network catalogs

`cdn_catalog.json` is the complete regional host snapshot from `Kanda-Akihito-Kun/ccb` file `data/cdn.json` at commit `a8aecab2b0c270149f5b205e90ffd7e4c6f80e24`. The snapshot is included as data; host availability is not guaranteed.

The source file is distributed under the MIT License. Its copyright and license notice are reproduced here:

```text
MIT License

Copyright (c) 2025 鼠鼠今天吃嘉然

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

`resolver_catalog.json` contains public resolver host and region facts transcribed from `10miaomiao/bilimiao2` file `doc/区域限制-代理服务器设置.md` at repository revision `8e8806d1a4c37de4c5deab1e9ef7356610aa6e33`. This is a historical, unverified list. Entries are opt-in and no resolver is selected by default. The source page warns that availability and compatibility are not guaranteed.

`crates/bbdown/src/category_catalog.json` contains a dated factual snapshot of Bilibili's public
frontend `channelList` fallback menu. It was transcribed from the page and statically inspected
frontend asset listed below; it does not contain copied frontend JavaScript source code and is not
claimed under the CDN catalog's MIT notice. The frontend can replace this fallback from a remote
key-value menu at runtime, so the snapshot is not a claim about the current effective menu or a
complete category taxonomy. The snapshot records `captured_at=2026-10-22T22:32:44.646206Z`,
`source_url=https://www.bilibili.com/c/douga/`, and
`source_asset_url=https://s1.hdslb.com/bfs/static/shanks/channel/assets/index-bedd2c00.js` with
SHA-256 `e9a6380a51acb054d8728db716a10a13818371a7834ba5f4f53a4b7872cfcbcb`. The independent
`sideChannelList` is not included.
