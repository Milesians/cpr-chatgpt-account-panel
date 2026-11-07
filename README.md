# ChatGPT 账号面板（CPR 插件）

[Codex Proxy RS](https://github.com/zyycn/codex-proxy-rs)（CPR）的插件：在 CPR 的“插件”导航里按账号查看 OpenAI OAuth 账号的 **ChatGPT 官方用量**（按使用位置，重点看 **Work** 有没有用量）、**订阅与账单**。登录（用登录凭据换令牌、快速导入）在 `login` 模块，单独维护，见[模块分工](#模块分工)。

插件 ID `milesians.cpr-chatgpt-account-panel`（发布者 `milesians`），要求 CPR `>=3.34.0, <4.0.0`（清单 v2、RPC v7、`management` v2）。SDK 和打包 CLI 都固定在 CPR 的 `v3.34.0`（`47d30474af7c3e1fab433f2d4a70430fae548ff2`），和 [cpr-turnstate](https://github.com/Milesians/cpr-turnstate) 一致。

## 怎么读

- 每次查询前经 `host.auth.get` 现读这个账号的凭据，只放内存；经 `host.http.account` 走账号在 CPR 绑定的代理（没绑就直连；绑了但不可用就失败，不回退直连），带凭据版本号，凭据刚变就失败、让人重试
- **只发 GET**，不跟随跳转，不刷新令牌（访问令牌过期或 30 秒内过期就不查，等 CPR 自己刷新），不改 CPR 里的账号
- 请求头只有 `Authorization`、`Accept`、`User-Agent`、`Originator`、`Chatgpt-Account-Id`。UA 和 Originator 实时取 CPR 的 Codex 客户端身份：宿主的 TLS 栈是 rustls，和 Codex CLI 一致，不冒充浏览器
- 停用的账号、CPR 标记 expired/banned/invalid 的账号、缺 ChatGPT 账号 ID 的账号不查；API Key 账号不列出
- 令牌被拒（`token_revoked` 单独说明）、Cloudflare 挑战页、限流时整次查询停下，不再发后面的请求；回答里的 `account_id` 和这个账号对不上就拒绝（`account_mismatch`）
- 上游拒绝的原因只进宿主日志（抹掉令牌），给页面的是固定的中文说明

## 官方用量

点了才查，读 Codex 用量页和 Codex CLI `/usage` 用的内部接口（路由见 openai/codex 的 `codex-rs/backend-client`）：

| 接口 | 内容 |
| --- | --- |
| `/backend-api/wham/usage` | 当前额度窗口，按 `window_minutes` 命名和排序（300 是 5 小时、10080 是每周；Pro 可能只有每周窗口） |
| `/backend-api/wham/profiles/me` | 每天的 Token、累计、对话数 |
| `/backend-api/wham/usage/daily-token-usage-breakdown`（Team 读 `daily-workspace-user-token-usage-breakdown`） | 每天按使用位置、发起方式、模型的分布；个人套餐是相对用量（占额度的比例），Team 是 credits |

- **Work**：ChatGPT 按请求方报的客户端身份记使用位置。CPR 以 Codex 的身份转发，所以记在 `work`、`work_web`、`work_desktop`、`work_mobile` 下面的用量**一定不是经 CPR 用的**。页面单独算出 Work 的用量、占比、有用量的天数和最近一天，图上 Work 的几个位置合成一种颜色
- 三部分各读各的，读不到的部分说明原因、其余照常；位置、模型等名字只放行简单字符，其余归“其他”
- 每次查询的摘要（Work、各位置占比、额度、失败原因）记在插件私有状态 `usage` 里，列表一直显示，直到下次查询

## 订阅与账单

点了才查：

- `/backend-api/subscriptions?account_id=`：套餐、有效状态、到期、自动续费；付款逾期时读 `became_delinquent_timestamp` 和 `grace_period_end_timestamp`（RFC 3339 或 Unix 秒，别的格式只丢那一项）
- `/backend-api/payments/transaction-history?account_id=&limit=20`：账单分页。第一页最新一笔是扣款失败类（`failed`、`past_due`、`unpaid`…）时顺带查一次订阅，显示宽限期
- 查第一页时把最新一笔（日期、金额、状态、套餐，不含发票链接）记在插件私有状态 `payments` 里，列表显示“最近支付”
- 发票只给 HTTPS 的 `invoice.stripe.com/i/` 链接。插件页面在 CPR 的隔离 iframe 里（`sandbox="allow-scripts"`），不能开新标签，所以页面给出链接让人复制

## 页面

- **列表**：每个 OpenAI OAuth 账号一行：账号（邮箱、套餐、个人/Team、查不了的原因）、CPR 自己记下的额度（`host.data.quota.get`，不访问上游）、最近一次官方用量（各位置占比条、Work 占比）、最近支付。Work 有用量的排最前，其次周额度超过 90% 的、没查过或查失败的。可按个人/Team、“Work 有用量”“扣款异常”筛选、搜索
- **查询全部用量 / 查询全部账单**：一个接一个查（用量查最近 7 天，账单查第一页），可以中途停；跳过现在查不了的账号
- **账号详情**：官方用量（7/14/30 天，额度窗口、Work 提示、每天按位置的堆叠图、位置/发起方式/模型占比、每日 Token）和订阅与账单

使用位置的颜色跟着位置走、不跟名次，浅色和深色都用 dataviz 校验脚本检查过（浅色下有 3 个颜色对底色不到 3:1，所以每处都有文字图例和数值）。

## 模块分工

- **面板**（用量、账单、列表、详情）：`backend/src/{chatgpt,usage,billing,store,management,api,host}.rs`、`frontend/src/{App.vue,api.ts,format.ts,surfaces.ts,components/}`
- **登录**（用登录凭据换令牌并写回 CPR、快速导入新账号）：`backend/src/login/`、`frontend/src/login/`，单独维护。和面板的接口：
  - 后端：`login::routes()` 返回本模块的管理路由（路径都以 `api/login/` 开头，`management` 合进注册），`api/login/` 开头的请求交给 `login::handle`；`login::Login` 是进程内状态，用 `Config` 建一次。错误用 `api::ApiError`，路由用 `api::route`
  - 前端：`login/LoginPage.vue` 是顶部“登录与导入”页，`login/AccountLogin.vue` 放在账号详情页最下面（props `accountId`）。请求用 `api.ts` 的 `call`
  - 设置项加在 `config::Config` 和 `plugin.json` 的 `configurationSchema`（敏感的进 `secretFields`）；私有状态的命名空间加在 `plugin.json` 的 `state`
  - 共用的：`chatgpt::supported` / `chatgpt::is_team`（个人号和 Team 按 CPR 给的套餐区分）、`host.rs` 的宿主回调封装

## 开发

需要 Rust 1.99.1、Node.js 24、pnpm 12.6。

```bash
pnpm --dir frontend install --frozen-lockfile
pnpm --dir frontend build
pnpm --dir frontend test
cargo test --manifest-path backend/Cargo.toml
```

`backend/tests/panel.rs` 经公开 RPC 会话驱动插件、模拟宿主回调，检查只发 GET、走账号代理并带凭据版本号、请求头、令牌不进响应/私有状态/日志、遇到拒绝就停、账号对不上就拒绝、列表不读凭据。

## 打包与安装

```bash
cargo install --locked --git https://github.com/zyycn/codex-proxy-rs.git --rev 47d30474af7c3e1fab433f2d4a70430fae548ff2 codex-proxy-plugin-cli --root .tools
bash scripts/package            # 默认本机平台；也可传 aarch64-unknown-linux-gnu 等
```

在 CPR 的“插件管理”点安装：上传 `dist/` 里的 `.tar.gz`，或来源选 GitHub。发布新版本：改 `plugin.json` 的 `version` 和 `release/notes.md`，推送 `v<版本>` 标签，GitHub Actions 检查通过后构建 Linux x86_64 / aarch64 两个安装包并发布。插件以 CPR 的系统身份运行（能读账号凭据），CPR 不提供沙箱。

## 许可

MIT。
