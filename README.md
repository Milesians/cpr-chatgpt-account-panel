# ChatGPT 账号面板（CPR 插件）

[Codex Proxy RS](https://github.com/zyycn/codex-proxy-rs)（CPR）的插件：在 CPR 的“插件”导航里按账号查看 OpenAI OAuth 账号的 **ChatGPT 官方用量**（按使用位置，重点看 **Work** 有没有用量）、**订阅与账单**和**登录设备**。登录（用登录凭据换令牌、快速导入）在 `login` 模块，单独维护，见[模块分工](#模块分工)。

插件 ID `milesians.cpr-chatgpt-account-panel`（发布者 `milesians`），要求 CPR `>=3.34.0, <4.0.0`（清单 v2、RPC v7、`management` v2）。SDK 和打包 CLI 都固定在 CPR 的 `v3.34.0`（`47d30474af7c3e1fab433f2d4a70430fae548ff2`），和 [cpr-turnstate](https://github.com/Milesians/cpr-turnstate) 一致。

## 怎么读

- 每次查询前经 `host.auth.get` 现读这个账号的凭据，只放内存；经 `host.http.account` 走账号在 CPR 绑定的代理（没绑就直连；绑了但不可用就失败，不回退直连），带凭据版本号，凭据刚变就失败、让人重试
- **只发 GET**（唯一的例外是管理员点了的登出设备，见下），不跟随跳转，不刷新令牌（访问令牌过期或 30 秒内过期就不查，等 CPR 自己刷新），不改 CPR 里的账号
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
- 每次查询的摘要（每日 Token、各位置占比、额度、失败原因，详情页用的 Work 统计）记在插件私有状态 `usage` 里，列表一直显示，直到下次查询

## 订阅与账单

点了才查：

- `/backend-api/subscriptions?account_id=`：套餐、有效状态、到期、自动续费；付款逾期时读 `became_delinquent_timestamp` 和 `grace_period_end_timestamp`（RFC 3339 或 Unix 秒，别的格式只丢那一项）
- `/backend-api/payments/transaction-history?account_id=&limit=20`：账单分页。第一页最新一笔是扣款失败类（`failed`、`past_due`、`unpaid`…）时顺带查一次订阅，显示宽限期
- 查第一页时把最新一笔（日期、金额、状态、套餐，不含发票链接）记在插件私有状态 `payments` 里，列表显示“最近支付”
- 发票只给 HTTPS 的 `invoice.stripe.com/i/` 链接。插件页面在 CPR 的隔离 iframe 里（`sandbox="allow-scripts"`），不能开新标签，所以页面提供“复制发票链接”按钮让人复制

## 登录设备

点了才查：`GET /backend-api/accounts/sessions` 列出 ChatGPT 设置页里这个登录的设备，`POST /backend-api/accounts/sessions/revoke` 登出。都是 ChatGPT 的内部接口，回答不像设置页的（有分页、缺 `is_current_device`、会话 ID 重复或不合法）就拒绝，不猜。

- 会话 ID 不出插件：页面只拿它的哈希；登出前重新列一次、按哈希找，只能登出现在列着的设备，登出后再列一次确认
- CPR 令牌所在的会话（`is_current_device`，或令牌 claims 里的 `session_id`）不能单独登出
- **登出其他设备**：只有唯一认出了 CPR 的会话时才能做；每登出一台前都重新核对列表，出现新设备或 CPR 的会话不在了就停
- **登出所有设备**：连 CPR 的会话一起登出，放在最后（请求用的就是它的令牌），期间新登录的设备也登出；之后 CPR 里这个账号的令牌失效，要重新登录写回 CPR。没认出 CPR 的会话、令牌中途失效的报 `cpr_voided_early`
- 单台“登出”和“登出其他设备”直接执行，只有“登出所有设备”点之前在应用内确认一次；中途停下时错误里说明已经登出了几台

## 页面

- **列表**：每个 OpenAI OAuth 账号一行：账号（邮箱、套餐与类型、无法查询的原因）、CPR 额度（`host.data.quota.get`，不访问上游）、最近一次官方用量（每日 Token 柱状图和合计，悬停或点按柱子看当天详情；使用位置占比饼图，悬停看各位置的占比）、最近支付。列表不对 Work 做特殊处理：周额度超过 90% 的排最前，其次没查过或查失败的，也可按 Token 用量排序。可按个人/Team、“扣款异常”“周额度 >90%”“未查询”“无法查询”筛选、搜索。令牌过期或凭据异常的行在操作区直接提供“免输重登”（已存密码）或“去重新登录”（未存密码）
- **查询全部用量 / 查询全部账单 / 批量免输重登**：用量和账单同时查 6 个账号（各走自己绑定的代理），批量免输重登一个接一个（用量查最近 7 天，账单查第一页，批量免输重登针对已选中有已存密码的启用账号），可以中途停；跳过现在无法执行的账号并说明原因
- **账号详情**：单页上下依次展示官方用量（7/14/30 天，“查询用量”/“刷新”，额度窗口、Work 位置明细、每天按位置的堆叠图、位置/发起方式/模型占比、每日 Token）、订阅与账单（“查询订阅与账单”/“刷新”并行拉取）、重新登录（“免输重登”/“登录并写回 CPR”、“仅保存”、“清除已存凭据”、“浏览器手动登录”）、登录设备（“查询设备”/“刷新”）

使用位置的颜色跟着位置走、不跟名次，浅色和深色都用 dataviz 校验脚本检查过（浅色下有 3 个颜色对底色不到 3:1，所以每处都有文字图例和数值）。

## 重新登录与导入账号

- **凭据存储**：账号的密码与 2FA 密钥（Base32 或 `otpauth://totp/` 链接）以**明文**存在 CPR 数据库 `plugin_state_records` 表的 `login` 命名空间里（键为 CPR 账号 ID；粘贴待导入时暂存在 `email:<邮箱>:<kind>` 下）。密码与 2FA 绝不写入宿主日志 `host.log` 或错误信息，前端页面也绝不回显明文（只返回 `hasPassword` / `hasTotp` 显示“已保存密码 / 已保存 2FA”或“未存密码 / 未存 2FA”）。库里若有旧版加密字段（`passwordEnc` / `totpEnc`），反序列化时自然当作未保存密码（`None`）。
- **已有账号重登**：经该账号在 CPR 绑定的出站代理（`host.http.account`）跑完 OpenAI OAuth PKCE 登录状态机（含 Sentinel PoW、TOTP 验证、Workspace 选择）并换取新令牌，经 `host.auth.save`（`Replace`）写回 CPR。
- **导入账号（复用借道账号代理）**：
  1. 粘贴文本后自动防抖预览（统计总行数、待新建、已在 CPR、跳过），点“导入 N 个”逐条执行。
  2. 先经 CPR 管理接口 `GET /api/admin/accounts` 读取所选借道账号（carrier）绑定的 `outboundProxyId`；若借道账号未绑定出站代理则直接拒绝导入（不创建直连账号）。
  3. 调 CPR 管理接口 `POST /api/admin/accounts/oauth/start`（不带 `accountId`，带 `outboundProxyId`，`name` 为邮箱），获取 CPR 生成的授权链接与 `flowId`。
  4. 借道账号经 `host.http.account` 跑完登录步骤，截下回调地址 `callbackUrl` 后交给 `POST /api/admin/accounts/oauth/complete`，由 CPR 自己通过该代理换码并一步建好已绑定代理的新账号（套餐由 CPR 自己从令牌解析，不调 `host.auth.save Create`）。
  5. 建号完成后核对新账号邮箱，将明文密码与 2FA 保存到新账号 ID 下，并用 `host.state.delete`（带 `expectedVersion`）删掉 `email:<邮箱>:<kind>` 暂存记录。

## 模块分工

- **面板**（用量、账单、登录设备、列表、详情）：`backend/src/{chatgpt,usage,billing,devices,store,management,api,host}.rs`、`frontend/src/{App.vue,api.ts,format.ts,surfaces.ts,components/}`
- **登录**（用登录凭据换令牌并写回 CPR、导入新账号）：`backend/src/{admin.rs,login/}`、`frontend/src/login/`，单独维护。和面板的接口：
  - 后端：`login::routes()` 返回本模块的管理路由（路径都以 `api/login/` 开头，`management` 合进注册），`api/login/` 开头的请求交给 `login::handle`；`login::Login` 是进程内状态，用 `Config` 建一次。错误用 `api::ApiError`，路由用 `api::route`
  - 前端：`login/LoginPage.vue` 是顶部“导入账号”页，`login/AccountLogin.vue` 放在账号详情页（props `accountId`）。请求用 `api.ts` 的 `call`
  - 设置项加在 `config::Config` 和 `plugin.json` 的 `configurationSchema`（普通设置 `cprAdminUrl`、`oauthRedirect`，敏感设置 `cprAdminApiKey` 进 `secretFields`）；私有状态的命名空间加在 `plugin.json` 的 `state`
  - 共用的：`chatgpt::supported` / `chatgpt::is_team`（个人号和 Team 按 CPR 给的套餐区分）、`host.rs` 的宿主回调封装

## 开发

需要 Rust 1.99.1、Node.js 24、pnpm 12.6。

```bash
pnpm --dir frontend install --frozen-lockfile
pnpm --dir frontend build
pnpm --dir frontend test
cargo test --manifest-path backend/Cargo.toml
```

`backend/tests/panel.rs` 和 `devices.rs` 经公开 RPC 会话驱动插件、模拟宿主回调，检查只发 GET（登出设备的顺序、核对和停下的条件另测）、走账号代理并带凭据版本号、请求头、令牌不进响应/私有状态/日志、遇到拒绝就停、账号对不上就拒绝、列表不读凭据。

## 打包与安装

```bash
cargo install --locked --git https://github.com/zyycn/codex-proxy-rs.git --rev 47d30474af7c3e1fab433f2d4a70430fae548ff2 codex-proxy-plugin-cli --root .tools
bash scripts/package            # 默认本机平台；也可传 aarch64-unknown-linux-gnu 等
```

在 CPR 的“插件管理”点安装：上传 `dist/` 里的 `.tar.gz`，或来源选 GitHub。发布新版本：改 `plugin.json` 的 `version` 和 `release/notes.md`，推送 `v<版本>` 标签，GitHub Actions 检查通过后构建 Linux x86_64 / aarch64 两个安装包并发布。插件以 CPR 的系统身份运行（能读账号凭据），CPR 不提供沙箱。

## 许可

MIT。
