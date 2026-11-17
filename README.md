# ChatGPT 账号面板（CPR 插件）

[Codex Proxy RS](https://github.com/zyycn/codex-proxy-rs)（CPR）的插件：在 CPR 的“插件”导航里统一管理 OpenAI OAuth 账号，查看 **ChatGPT 官方用量**（重点看 **Work**）、**订阅与账单**、**登录设备**和**探测记录**，也可以导入或重新登录账号。

插件 ID `milesians.cpr-chatgpt-account-panel`（发布者 `milesians`），要求 CPR `>=3.44.0, <4.0.0`（清单 v2、RPC v7、`management` v2；导入和重登要用 3.44.0 新增的 `host.http.proxy` 和 `host.proxies.list`）。SDK 和打包 CLI 都固定在 CPR 的 `v3.44.0`（`c622f085748f812936a7371fcc85d1621fa72ca8`）。

## 怎么读

- 每次查询前经 `host.auth.get` 现读这个账号的凭据，只放内存；经 `host.http.account` 走账号在 CPR 绑定的代理（没绑就直连；绑了但不可用就失败，不回退直连），带凭据版本号，凭据刚变就失败、让人重试
- **查询用量、账单和设备只发 GET**（管理员点了登出设备时才发 POST），不跟随跳转，不刷新令牌（访问令牌过期或 30 秒内过期就不查，等 CPR 自己刷新），不改 CPR 里的账号
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

- `/backend-api/subscriptions?account_id=`：套餐、有效状态、到期、自动续费；付款逾期时读 `became_delinquent_timestamp` 和 `grace_period_end_timestamp`（RFC 3339 或 Unix 秒/毫秒，别的格式只丢那一项）
- `/backend-api/payments/transaction-history?account_id=&limit=20`：账单分页。查第一页时一并查订阅（无发票权限的 Team 成员也能读到订阅与宽限期）
- 查第一页或单独查订阅时把最近支付、订阅状态、到期时间、续费与宽限期（不含发票链接）记在插件私有状态 `payments` 里；老记录没有订阅状态时，点「查询全部订阅」补齐
- 发票只给 HTTPS 的 `invoice.stripe.com/i/` 链接。插件页面在 CPR 的隔离 iframe 里（`sandbox="allow-scripts"`），不能开新标签，所以页面提供“复制发票链接”按钮让人复制

## 登录设备

点了才查：`GET /backend-api/accounts/sessions` 列出 ChatGPT 设置页里这个登录的设备，`POST /backend-api/accounts/sessions/revoke` 登出。都是 ChatGPT 的内部接口，回答不像设置页的（有分页、缺 `is_current_device`、会话 ID 重复或不合法）就拒绝，不猜。

- 会话 ID 不出插件：页面只拿它的哈希；登出前重新列一次、按哈希找，只能登出现在列着的设备，登出后再列一次确认
- CPR 令牌所在的会话（`is_current_device`，或令牌 claims 里的 `session_id`）不能单独登出
- **登出其他设备**：只有唯一认出了 CPR 的会话时才能做；每登出一台前都重新核对列表，出现新设备或 CPR 的会话不在了就停
- **登出所有设备**：连 CPR 的会话一起登出，放在最后（请求用的就是它的令牌），期间新登录的设备也登出；之后 CPR 里这个账号的令牌失效，要重新登录写回 CPR。没认出 CPR 的会话、令牌中途失效的报 `cpr_voided_early`
- 单台“登出”和“登出其他设备”直接执行，只有“登出所有设备”点之前在应用内确认一次；中途停下时错误里说明已经登出了几台

## 页面

- **账号列表**：降智判定、测票时间条、正常率和订阅状态在同一行。探针默认收成细条，点时间格看记录；「展开探针」可以放大时间条。可切换 24 小时、7 天或 30 天，按降智、订阅异常、宽限期筛选，或按宽限期到期排序。
- **查询订阅与批量操作**：「查询全部订阅」只查订阅；勾选账号后可批量探测、配置、查订阅、查账单、查用量或免输重登。订阅、账单和用量同时查最多 6 个账号；重登逐个执行，跳过不符合条件的账号。
- **账号详情**：点账号看探测记录，点订阅状态看订阅与账单。同一个详情页可切换到官方用量（Work、每日 Token 和使用位置）或登录与设备。列表和详情都不展示额度窗口。

## 账号探测

从统一账号列表可直接打开探测详情；「设置」管理自动分组、通知和指纹库。测票沿用账号在 CPR 绑定的代理；ModelTrace 走 CPR 模型链，需要填写 `modeltraceClientKeyId`。测票连续三次确认判定变化，可触发 Telegram 通知和自动分组。指纹库同步和 Telegram 通知继续直连。

- **发出真实探测**：默认关。打开后才能手动实际发出测票或 ModelTrace，会消耗账号额度。
- **运行探测后台任务**：默认关。打开后才会自动测票、执行账号失效自动重登、监控凭据异常、按判定调整分组、重试待发通知和定期同步指纹库。自动测票还需要打开真实探测，并给账号开启自动测票。

## 从独立账号探测插件迁移

从 `milesians.cpr-turnstate` v0.2.4 迁到原 `milesians.cpr-chatgpt-account-panel` 实例：

1. **先就地升级账号面板**，成功打开一次新版。先别在新版发起探测、改探测规则或开启后台任务。不要卸载旧探测插件，历史还在旧实例下。
2. 在 CPR 中停用旧探测和新版面板两个实例，然后停止 CPR 服务（保留 PostgreSQL 运行），备份 CPR 数据库。以下查询用来找**实例 UUID**；存在多个同名实例时，按名称核对源和目标：

   ```sql
   SELECT i.id, i.name, a.plugin_id, a.version, i.enabled
   FROM plugin_instances i JOIN plugin_artifacts a ON a.sha256 = i.artifact_sha256
   WHERE a.plugin_id IN ('milesians.cpr-turnstate', 'milesians.cpr-chatgpt-account-panel');
   ```

3. 在服务器上用连接到 CPR PostgreSQL 的 `psql` 执行 [迁移脚本](scripts/migrate-turnstate.sql)（以下假定已配置 `PGHOST`、`PGDATABASE`、`PGUSER` 等连接参数）。先预演，核对各命名空间记录数和自建分组数，再提交：

   ```bash
   psql -X -v source=旧探测实例UUID -v target=账号面板实例UUID -f migrate-turnstate.sql
   psql -X -v source=旧探测实例UUID -v target=账号面板实例UUID -v apply=true -f migrate-turnstate.sql
   ```

4. 启动 CPR，**只启用新版账号面板**。核对判定、历史、每账号的自动测票配置和分组规则，再在面板插件设置里开启「运行探测后台任务」。旧探测实例保留停用，避免重复探测或通知。

脚本复制自动探测配置、判定、历史和待发通知，搬入探测设置及 Telegram 敏感配置。自建分组只转移归属，分组 ID、成员关系和 Client Key 绑定不变。面板原有用量、账单、登录状态和 OAuth 配置保留。脚本默认回滚，正式迁移拒绝覆盖已存在的探测状态。若要完整回退（包括恢复自建分组归属），用迁移前的数据库备份；请在开启新版后台前做好核验。

## 重新登录与导入账号

- **凭据存储**：账号的密码与 2FA 密钥（Base32 或 `otpauth://totp/` 链接）以**明文**存在 CPR 数据库 `plugin_state_records` 表的 `login` 命名空间里（键为 CPR 账号 ID；粘贴待导入时暂存在 `email:<邮箱>:<kind>` 下）。密码与 2FA 绝不写入宿主日志 `host.log` 或错误信息，前端页面也绝不回显明文（只返回 `hasPassword` / `hasTotp` 显示“已保存密码 / 已保存 2FA”或“未存密码 / 未存 2FA”）。库里若有旧版加密字段（`passwordEnc` / `totpEnc`），反序列化时自然当作未保存密码（`None`）。
- **已有账号重登**：跑完 OpenAI OAuth PKCE 登录状态机（含 Sentinel PoW、TOTP 验证、Workspace 选择）并换取新令牌，经 `host.auth.save`（`Replace`）写回 CPR。出口和 CPR 用这个账号时一致：
  - 绑着启用的已保存代理：经管理接口 `GET /api/admin/accounts/detail` 读出代理 ID，按 ID 经 `host.http.proxy` 发，不带账号凭据版本，CPR 中途刷新令牌不会打断登录
  - 没绑代理、绑的代理已停用（CPR 这时直连）、用内联代理地址，或读不到绑定：经账号自己的 `host.http.account`
  - 停用的账号不能重登
- **自动重登**：在账号详情单独开启，要求已经保存登录密码。CPR 标记凭据过期/失效、访问令牌到期，或探测/查询返回 401 时，后台任务用该账号已保存的密码与 2FA 自动登录一次；同一份失效凭据不会重复尝试。成功和失败都会进入 TG 持久通知队列，账号详情同时显示最近一次触发原因与结果。该功能要求打开「运行探测后台任务」；TG 结果通知还要求配置并打开「TG 状态通知」。
- **导入账号（直连或选代理）**：
  1. 粘贴文本后自动防抖预览（统计总行数、待新建、已在 CPR、跳过），为新账号选直连或 CPR 代理，点“导入 N 个”逐条执行。
  2. 代理列表经宿主的 `host.proxies.list` 读出，只有代理的 ID、名称和是否启用，没有地址和认证。停用的代理不能选。
  3. 选直连时，自动登录请求由 CPR 服务器直接发出；选代理时经 `host.http.proxy` 按 ID 发出，代理无效时不会回退直连。两种方式都不借现有账号。
  4. 调 CPR 管理接口 `POST /api/admin/accounts/oauth/start`（不带 `accountId`，`outboundProxyId` 为空字符串表示直连，`name` 为邮箱）。自动登录或浏览器手动登录得到回调地址后，交给 `POST /api/admin/accounts/oauth/complete`，由 CPR 按所选出口换码并创建账号，直连账号不绑定代理（不调 `host.auth.save Create`）。
  5. 建号完成后核对新账号邮箱，将明文密码与 2FA 保存到新账号 ID 下，并用 `host.state.delete`（带 `expectedVersion`）删掉 `email:<邮箱>:<kind>` 暂存记录。
- **CPR 管理接口**：经 `host.http.dispatch` 调宿主自己的路由（只传 `/api/admin/...` 相对路径），宿主给子请求注入插件身份，不需要管理 API Key，也不出本机网络。v0.1.5 起清单里不再有 v0.1.3 及以前的 `cprAdminUrl`、`cprAdminApiKey`，从旧版本升级时按发布说明删掉它们。
- **多个工作区**：账号有多个工作区或组织时，错误里带上候选（`details.choices`），详情页和导入行直接点选重试，不用手抄 ID。
- **2FA**：当前 30 秒窗口只剩 4 秒以内时，等到下一个窗口再生成验证码，避免经代理到达时已过期；“发起 2FA 验证失败”（`mfa_issue_failed`）和“验证码被拒”（`totp_rejected`）分开报。

## 模块分工

- **面板**（用量、账单、登录设备、列表、详情）：`backend/src/{chatgpt,usage,billing,devices,store,management,api,host}.rs`、`frontend/src/{App.vue,api.ts,format.ts,surfaces.ts,components/}`
- **探测**（测票、ModelTrace、历史、调度、通知和分组）：`backend/src/probe/`、`frontend/src/probe/`。路由在 `api/probe/`；维护由 CPR 的 `plugin.reconcile` 触发。
- **登录**（用登录凭据换令牌并写回 CPR、导入新账号）：`backend/src/{admin.rs,login/}`、`frontend/src/login/`，单独维护。和面板的接口：
  - 后端：`login::routes()` 返回本模块的管理路由（路径都以 `api/login/` 开头，`management` 合进注册），`api/login/` 开头的请求交给 `login::handle`；`login::Login` 是进程内状态，用 `Config` 建一次。错误用 `api::ApiError`，路由用 `api::route`
  - 前端：`login/LoginPage.vue` 是顶部“导入账号”页，`login/AccountLogin.vue` 放在账号详情页（props `accountId`）。请求用 `api.ts` 的 `call`
  - 设置项加在 `config::Config` 和 `plugin.json` 的 `configurationSchema`（现在只有 `oauthRedirect`）；私有状态的命名空间加在 `plugin.json` 的 `state`
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
cargo install --locked --git https://github.com/zyycn/codex-proxy-rs.git --rev c622f085748f812936a7371fcc85d1621fa72ca8 codex-proxy-plugin-cli --root .tools
python3 -m venv .tools/zig && .tools/zig/bin/pip install ziglang==0.15.2 cargo-zigbuild==0.23.4
export PATH="$PWD/.tools/zig/bin:$PATH"   # cargo-zigbuild 经这个环境的 python 找到 Zig
bash scripts/package            # 默认本机平台；也可传 aarch64-unknown-linux-gnu 等
```

Linux 安装包用 `cargo zigbuild --target <平台>.2.28` 构建，二进制最高只依赖 `GLIBC_2.28`：CPR 官方镜像是 Debian 12（glibc 2.36），在新系统上直接 `cargo build` 会链接更高的 GLIBC 符号，插件进程在握手前就退出。没装 cargo-zigbuild 时脚本退回 `cargo build` 并警告；发版流程设了 `REQUIRE_GLIBC_FLOOR=1`，缺工具或符号超过 2.28 就失败。

在 CPR 的“插件管理”点安装：上传 `dist/` 里的 `.tar.gz`，或来源选 GitHub。发布新版本：改 `plugin.json` 的 `version` 和 `release/notes.md`，推送 `v<版本>` 标签，GitHub Actions 检查通过后构建 Linux x86_64 / aarch64 两个安装包并发布。插件以 CPR 的系统身份运行（能读账号凭据），CPR 不提供沙箱。

## 许可

MIT。ModelTrace 算法和内置指纹库来自 xqy2006/ModelTrace（MIT），内置 Codex 提示词来自 openai/codex（Apache-2.0）。来源和许可证见 [`backend/data/`](backend/data/)。
