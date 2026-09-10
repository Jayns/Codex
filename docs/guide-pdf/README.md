# 便携版使用说明 PDF 构建

从 `docs/` 下的 Markdown 生成排版后的 PDF：

- [`../ChatGPT便携版使用说明.md`](../ChatGPT便携版使用说明.md) → `output/pdf/ChatGPT便携版使用说明.pdf`
- [`../ChatGPT便携版使用说明-macOS.md`](../ChatGPT便携版使用说明-macOS.md) → `output/pdf/ChatGPT便携版使用说明-macOS.pdf`

## 用法

```bash
bash docs/guide-pdf/build.sh            # 两个平台都出
bash docs/guide-pdf/build.sh windows    # 只出 Windows 版
bash docs/guide-pdf/build.sh macos      # 只出 macOS 版
```

产物在 `output/pdf/`（`output/` 已在 `.gitignore` 中，不入库）。

## 依赖

| 工具 | 说明 |
| --- | --- |
| `pandoc` | Markdown → 自包含 HTML（图片内嵌为 data URI） |
| Chrome / Chromium / Edge | HTML → PDF（headless `--print-to-pdf`）。脚本会自动查找；也可用 `CHROME_BIN` 指定 |
| `python` + `pymupdf` | 给每页（封面除外）加页脚标题与页码 |

## 文件

- `build.sh` —— 构建脚本（pandoc → Chrome → footer.py）
- `style.css` —— 打印样式表，两个平台共用
- `cover-windows.html` / `cover-macos.html` —— 各自的封面 + 目录片段
- `footer.py` —— 页脚后处理（`python footer.py <in.pdf> <out.pdf> [标题]`）

## 排版约定

- 每个 `## 章节` 从新页开始；章节标题与其第一个 `### 步骤` 同页。
- 每个 `### 步骤` 从新页开始，标题 / 正文 / 截图始终同页（不跨页）。
  因此一个步骤最多带一张截图——需要两张时请拆成两个步骤。

## macOS 版的待补截图

见 [`../assets/chatgpt-usage-macos/README.md`](../assets/chatgpt-usage-macos/README.md)。
补图前 PDF 里那几步会显示占位符（图片加载失败），补齐同名文件后重跑脚本即可。
