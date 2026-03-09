# resumeclaw

通过聊天对话的方式编写和优化 LaTeX 简历的 Agent，支持 Discord / CLI 多渠道交互，修改后自动编译 PDF 并发送。

## 已支持的能力

- [x] 多渠道接入：Discord 机器人 + 本地 CLI
- [x] 多 LLM 后端：DeepSeek、OpenAI、Anthropic、Ollama、Groq、Together，支持自定义 OpenAI 兼容端点
- [x] Tool Calling 驱动的简历编辑：LLM 通过工具读取、修改 LaTeX 源文件
- [x] 自动编译：修改后调用 `xelatex` 编译为 PDF
- [x] PDF 自动发送：编译成功后通过 Discord 直接发送 PDF 文件
- [x] 独立工作区：简历模板和编辑文件存放在平台标准数据目录，不污染模板仓库
- [x] 代理支持：原生 HTTP 代理 / proxychains 外部代理两种模式

## 待完善的能力

- [ ] 提问式简历编写（Agent 主动向用户提问收集信息）
- [ ] 根据 JD 自动微调简历
- [ ] 简历版本管理与对比
- [ ] 简历自动投递及进度追踪
- [ ] 多语言简历支持（中/英切换）
- [ ] 更多渠道接入（飞书、Telegram 等）
- [ ] 对话上下文记忆（多轮跨消息）

## 快速开始

### 前置依赖

- Rust toolchain (cargo)
- XeLaTeX（macOS: `brew install --cask mactex`，Linux: `apt install texlive-xetex`）
- 简历模板仓库（默认从 `../resume` 读取，包含 `.cls`、字体等）

### 环境变量

创建 `.env` 文件或 export 环境变量：

```bash
# 必选 - LLM 配置
LLM_PROVIDER=deepseek          # deepseek / openai / anthropic / ollama / groq / together / custom
LLM_MODEL=deepseek-chat        # 模型名
DEEPSEEK_API_KEY=sk-xxx        # 对应 provider 的 API Key

# 必选 - Discord（不配则仅启用 CLI）
DISCORD_BOT_TOKEN=xxx

# 可选 - 路径
RESUME_TEMPLATE_DIR=../resume  # 简历模板目录，默认 ../resume
WORKSPACE_DIR=                 # 工作区目录，默认为平台标准路径（见下方说明）

# 可选 - 自定义 LLM 端点（LLM_PROVIDER=custom 时使用）
LLM_BASE_URL=https://your-endpoint.com
LLM_API_KEY=xxx
```

### 工作区路径

工作区存放编辑中的 `.tex` 文件和编译产物，默认路径：

| 平台    | 路径                                      |
| ------- | ----------------------------------------- |
| macOS   | `~/Library/Application Support/resumeclaw` |
| Linux   | `$XDG_DATA_HOME/resumeclaw`（默认 `~/.local/share/resumeclaw`） |
| Fallback | `~/.resumeclaw`                           |

首次启动会自动从模板目录复制 `.cls`、`.sty` 等支持文件，并 symlink 字体目录。

### 启动服务

```bash
cargo run
```

启动后可在 CLI 直接输入消息，或通过 Discord 与 Bot 对话。

## 本地集成测试

为了在不接入 Discord、也不调用真实大模型的情况下验证主流程，项目支持 `LLM_PROVIDER=mock` 的脚本化本地测试方案。这个方案参考了不少成熟开源项目常用的 **fixture / transcript 驱动测试** 思路：把模型输出固定成 JSON 脚本，让 Agent、工具调用、CLI 交互走真实代码路径，但外部依赖全部替换为本地可控输入。

### 运行自动化本地集成测试

```bash
cargo test --test local_integration
```

该测试会：

- 使用 CLI channel，而不是 Discord
- 使用 `mock` LLM provider 读取本地 JSON 脚本
- 初始化临时模板目录和工作区
- 驱动真实的 `read_resume` / `write_resume` 工具链

### 手动本地冒烟测试

准备一个 mock 脚本，例如：

```json
[
  {
    "tool_calls": [
      { "id": "call-read", "name": "read_resume", "arguments": {} }
    ]
  },
  {
    "content": "我已经读取完简历，可以继续下一步。",
    "tool_calls": []
  }
]
```

然后运行：

```bash
export LLM_PROVIDER=mock
export LLM_MODEL=mock-local
export MOCK_LLM_SCRIPT_PATH=/absolute/path/to/mock-llm.json
export RESUME_TEMPLATE_DIR=/absolute/path/to/your/template
export WORKSPACE_DIR=/absolute/path/to/your/workspace
cargo run
```

此时可以直接在 CLI 输入消息，验证 Agent 主流程，而无需配置 Discord Token 或真实模型 API Key。

## 代理配置

### 方式一：原生 HTTP 代理（仅 LLM API 走代理）

```bash
export https_proxy=http://127.0.0.1:1087
cargo run
```

> 注意：Discord Gateway 使用 WebSocket，不走 HTTP 代理。如果需要代理 Discord 连接，请使用方式二。

### 方式二：proxychains 全局代理（推荐）

所有 TCP 连接（包括 Discord WebSocket）统一由 proxychains 代理：

```bash
unset http_proxy https_proxy HTTP_PROXY HTTPS_PROXY
export PROXY_MODE=external
proxychains4 cargo run
```

`PROXY_MODE=external` 会：
1. 设置 `NO_PROXY=*` 禁用所有 HTTP 客户端（包括第三方库）的内置代理检测
2. 清除残留的代理环境变量
3. 让 proxychains 在 TCP 层统一处理所有网络连接

> **重要**：使用 proxychains 时必须 unset 代理环境变量，否则会导致双重代理（`TunnelUnexpectedEof` 错误）。

## Contributing

本项目目前处于开发初期，功能和架构都在快速迭代中。非常欢迎任何形式的贡献：Bug 报告、功能建议、代码 PR。

## 致谢

本项目受到以下开源项目的启发，在此表示感谢：

- [ironclaw](https://github.com/nearai/ironclaw) — Rust 实现的多渠道 AI Agent 框架，本项目的架构设计参考了其频道抽象和 LLM Provider 模式。MIT License.
- [resume](https://github.com/billryan/resume) — 简洁优雅的 LaTeX 中英文简历模板，本项目使用其模板类和字体配置作为简历编辑基础。MIT License.

## License

MIT
