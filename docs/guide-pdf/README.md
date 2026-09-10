# 便携版使用说明 PDF 构建

从 [`../ChatGPT便携版使用说明.md`](../ChatGPT便携版使用说明.md) 生成排版后的 PDF。

## 用法

```bash
bash docs/guide-pdf/build.sh
```

产物：`output/pdf/ChatGPT便携版使用说明.pdf`（`output/` 已在 `.gitignore` 中，不入库）。

## 依赖

| 工具 | 说明 |
| --- | --- |
| `pandoc` | Markdown → 自包含 HTML（图片内嵌为 data URI） |
| Chrome / Chromium / Edge | HTML → PDF（headless `--print-to-pdf`）。脚本会自动查找；也可用 `CHROME_BIN` 指定 |
| `python` + `pymupdf` | 给每页（封面除外）加页脚标题与页码 |

## 文件

- `build.sh` —— 构建脚本（pandoc → Chrome → footer.py）
- `style.css` —— 打印样式表（封面、目录、蓝色分节标题、一步一页、表格等）
- `cover.html` —— 封面 + 目录片段（`--include-before-body`）
- `footer.py` —— 页脚后处理（`python footer.py <in.pdf> <out.pdf> [标题]`）

## 排版约定

- 每个 `## 章节` 从新页开始；章节标题与其第一个 `### 步骤` 同页。
- 每个 `### 步骤` 从新页开始，标题 / 正文 / 截图始终同页（不跨页）。
  因此一个步骤最多带一张截图——需要两张时请拆成两个步骤。
