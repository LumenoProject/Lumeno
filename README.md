<p align="center">
  <img src="web/public/icon.png" width="144" alt="Lumeno 应用图标">
</p>

# Lumeno

<p align="center">
  <strong>简体中文</strong> · <a href="README.en.md">English</a>
</p>

Lumeno 是一个自我管理 Web 应用。它把想完成的事情写成明确的启动条件、动作和时限，并通过不可暂停的专注过程、连续记录与失败回溯，帮助你减少临场犹豫。

理论来源：本项目受到 Edmond 在知乎回答[《如何提高自制力？》](https://www.zhihu.com/question/19888447/answer/1930799480401293785)中所述方法的启发。

Lumeno 适合部署在个人服务器或家庭网络中，通过浏览器和安装后的 PWA 在电脑、平板与手机上使用。

> Lumeno 是行为记录与反思工具，不提供医疗诊断或治疗建议。如果专注、情绪、睡眠或冲动问题显著影响生活，请寻求合格专业人士帮助。

> [!WARNING]
> 本项目包含由 AI 生成或辅助生成的代码，可能存在错误、安全漏洞或未覆盖的边界情况。请在使用、部署或修改前自行审查代码并进行充分测试，尤其不要在未经评估的情况下直接用于生产环境。

## 主要功能

- 契约链：把任务写成启动条件、执行动作、完成时限和预约信号。
- 严格专注：开始后持续计时，不提供暂停；提前结束或中断必须完成判定。
- 任务群：把多个契约按顺序组成可嵌套的执行流程。
- 递归定式：从失败结果向前回溯，在国策库中试行、修订并逐日部署可重建的干预树。
- 国策组：用目标成员数和最低点亮数表达容错；成员失败时按门槛决定是否回滚组下游。
- 连续与记录：展示累计完成、失守、履约率和不可改写的协议时间线。
- 离线衔接：进行中的专注事实可在恢复网络后按顺序同步。
- 外部通知：支持 Bark 和自定义 Webhook，可选 Bark AES-256-GCM 内容加密。
- 自动时区：按当前浏览器时区计算每日定式激活边界。
- 响应式 PWA：支持桌面与移动浏览器、浅色/深色界面和安装到主屏幕。
- OIDC 登录：账号与密码由现有身份提供方管理。

## 快速开始

Lumeno 的默认部署包含应用、PostgreSQL 和 Caddy HTTPS。开始前需要：

- 一台已安装 Git、OpenSSL 和 Docker Compose 的服务器；
- 一个解析到服务器的域名，并开放 80/443 端口；
- 一个现有的 ZITADEL 或兼容 OIDC 身份提供方。

### 1. 获取项目并创建配置

```bash
git clone https://github.com/LumenoProject/Lumeno.git
cd Lumeno
cp deploy/.env.example deploy/.env
```

生成数据库密码和通知配置加密密钥：

```bash
openssl rand -hex 32
openssl rand -base64 32 | tr '+/' '-_' | tr -d '='
```

### 2. 填写环境变量

编辑 `deploy/.env`：

| 变量 | 用途 |
| --- | --- |
| `LUMENO_DOMAIN` | Lumeno 使用的域名，例如 `focus.example.com` |
| `LUMENO_PUBLIC_URL` | 完整站点根地址，例如 `https://focus.example.com/` |
| `OIDC_ISSUER` | 身份提供方的 issuer |
| `OIDC_CLIENT_ID` | OIDC Client ID |
| `OIDC_CLIENT_SECRET` | OIDC Client Secret |
| `POSTGRES_PASSWORD` | 内置 PostgreSQL 的强随机密码 |
| `NOTIFICATION_ENCRYPTION_KEY` | 用于加密已保存通知配置的服务端主密钥 |
| `NOTIFICATION_ENDPOINT_ALLOWLIST` | 允许接收通知的 HTTPS origin 白名单 |

将上一步的两个结果分别写入 `POSTGRES_PASSWORD` 和 `NOTIFICATION_ENCRYPTION_KEY`。`LUMENO_DOMAIN` 只填写域名，`LUMENO_PUBLIC_URL` 必须填写以 `/` 结尾的完整 HTTPS 地址。

随后在身份提供方中创建 Lumeno Web 应用，并登记登录回调、退出回调和后通道退出地址。具体配置见[通用 OIDC 指引](docs/OIDC.md)；ZITADEL 用户可直接阅读 [ZITADEL 配置指引](docs/ZITADEL.md)。

### 3. 启动并检查服务

```bash
docker compose --env-file deploy/.env -f deploy/docker-compose.yml pull
docker compose --env-file deploy/.env -f deploy/docker-compose.yml up -d
docker compose --env-file deploy/.env -f deploy/docker-compose.yml ps
```

所有服务显示为运行或健康后，在浏览器打开 `LUMENO_PUBLIC_URL` 即可登录。Caddy 会自动申请 HTTPS 证书，首次启动可能需要短暂等待。

## 开始使用

### 1. 建立第一条契约

进入“契约链”，点击“新建契约”，填写：

- 名称与说明；
- 能被清楚识别的启动条件；
- 启动后必须执行的动作；
- 需要持续的时间；
- 可选的预约信号、延迟时间与 90 秒启动窗口。

如果任务需要依次完成多个步骤，可以先建立任务群，再把叶子任务加入其中。

### 2. 从今日页面开始专注

“今日”页面会显示可执行契约、累计完成、失守和履约率。累计记录从不可改写的协议事件汇总，不会因归档或创建新修订而丢失。预约按时启动只更新预约连续，不重复计入完成次数；后续专注真正完成时才计为一次完成。选择一条契约并开始后，倒计时会持续流逝；刷新页面、切换页面或设备休眠都不会暂停时限。

倒计时结束后可以完成本次专注。若中断或提前结束，Lumeno 会要求确认这是一次失守，还是符合已经存在或新建立的允许判例。

点击开始或预约后会立即进入全屏倒计时，前 10 秒可无后果撤销；倒计时仍从最初点击时刻计算。撤销窗口结束后承诺正式生效，此后计时不可暂停。预约时间到达后会开启 90 秒启动窗口；窗口内启动会增加预约连续，未启动或主动终止则进入“失败或永久允许”的判定。主动终止前仍可返回倒计时；判定完成后预约状态立即释放，可以正常开始或再次预约。5 分钟内未确认会由服务器自动记为预约失败并清零预约连续。

契约归档后，首页的累计记录仍会保留。“契约链 → 查看归档”可以查看主动归档的契约，以及核心承诺修改前留下的旧修订。

### 3. 把失败写成下一次干预

进入“定式”，按照引导描述不可接受的结果、回溯决策链，并写下下一次可以识别和执行的国策。国策先保存在库中；只有当天完整试行成功后，才可消费这次试行记录点亮当前修订。每个本地自然日最多点亮一条国策，日期由账号时区在服务端计算。

国策定义、不可改写的修订和树中部署彼此分离：产生试行或部署历史后再编辑会创建新修订，旧部署仍保持当时的含义。普通国策失效时会熄灭自身和依赖后代，定义仍留在库中，可在以后重新试行和部署。累计存活天数按各轮实际生效秒数折算完整 24 小时，并不是习惯强度评分。

国策组把“成员”和“下游依赖”分开。活动成员达到最低点亮数后，组才能承载下游；成员失效后若仍达门槛，只熄灭该成员。跌破门槛时组进入降级状态并熄灭下游，但保留其他成员；补足成员后组会重新点亮，旧下游不会自动复活。上游失效时，整个组、成员和下游都会熄灭。

### 4. 查看协议记录

“记录”页面按时间展示启动、完成、失守和判定事实。记录不会被后续编辑覆盖，可以用来观察真正发生过的行为，而不是只依赖事后印象。

## 外部通知

使用通知前，部署者必须先在 `deploy/.env` 的 `NOTIFICATION_ENDPOINT_ALLOWLIST` 中加入允许的 HTTPS origin。未列入白名单的公网、内网和 Bark 地址都会被拒绝。

在“设置 → 外部通知”中选择通知方式：

- Bark：粘贴 Bark 首页提供的完整推送地址；可以启用内容加密，并在 Bark 客户端填写相同的 32 位密钥。
- 自定义 Webhook：填写 HTTPS 地址，并可选配置 Bearer Token。

预约、专注与任务群时限到达时，通知作业由后端服务器直接投递，不依赖网页保持打开或登录。通知地址和密钥保存后不会回填到浏览器；修改配置时需要重新填写完整内容。可以使用“测试通知”确认连接正常。

### 通知目标白名单

白名单使用逗号分隔，必须精确填写协议、主机和端口。官方 Bark 服务以及可信的自建 Bark 或 Webhook 可以这样配置：

```env
NOTIFICATION_ENDPOINT_ALLOWLIST=https://api.day.app,https://192.168.1.20:8443,https://bark.internal.example
```

不要在白名单中填写 Bark 设备密钥、Webhook 路径、查询参数或其他敏感内容。设置页仍填写完整推送地址，例如：

```text
https://192.168.1.20:8443/设备密钥
```

白名单 origin 必须与完整地址精确匹配；例如，`https://example.com` 不允许 `https://example.com:8443`。内网服务必须提供应用运行环境信任的 HTTPS 证书。如果部署环境使用出站代理，代理也必须能够访问该地址。

## PWA 与移动端

使用支持 PWA 的浏览器打开 Lumeno 后，可以通过浏览器菜单将其安装到桌面或手机主屏幕。移动端底部导航提供“今日、契约链、定式、记录、说明、设置”六个入口。

浏览器时区会在登录后自动同步，用于计算“每日一次点亮”和当天试行的本地日期边界。设备时区发生变化后，重新打开或刷新应用即可更新。

## 升级与备份

生产环境建议在 `deploy/.env` 中把 `LUMENO_IMAGE` 固定为已发布的版本标签或镜像 digest。升级时拉取新镜像并重新启动服务：

```bash
docker compose --env-file deploy/.env -f deploy/docker-compose.yml pull
docker compose --env-file deploy/.env -f deploy/docker-compose.yml up -d
```

需要验证 `dev` 分支时，可以手动运行仓库的 “Docker image” 工作流并选择 `dev` 渠道。它会发布 `ghcr.io/lumenoproject/lumeno:dev` 和对应的 `dev-sha-<短提交号>` 标签。`dev` 是会被后续构建覆盖的开发镜像，不用于生产部署；需要固定测试版本时应使用 `dev-sha-*` 标签或镜像 digest。

首次启动时，应用会在空数据库中自动初始化当前版本所需的结构。第二阶段的 RSIP 数据模型不提供旧版 `rsip_nodes` 数据回填或原地升级路径；已有旧版数据库不能直接套用本版本的初始迁移。用户、契约、记录和通知配置存放在 PostgreSQL 中；请定期备份 `postgres-data` 数据卷或外部数据库，并验证备份可以恢复。

如需使用外部 PostgreSQL，在 `deploy/.env` 中设置 `DATABASE_URL`，然后只启动应用和入口服务：

```bash
docker compose --env-file deploy/.env -f deploy/docker-compose.yml up -d --no-deps app caddy
```

## 理论与第三方归属

本项目受到 Edmond 在知乎回答[《如何提高自制力？》](https://www.zhihu.com/question/19888447/answer/1930799480401293785)中所述方法的启发，CTDP 与 RSIP 的产品语义源自该回答公开描述的方法。

实现设计同时参考了 Ken Xiao 的开源项目 [Momentum](https://github.com/Kapozux/Momentum)。Momentum copyright © 2025 Ken Xiao，原项目以 MIT License 发布。

Lumeno 是独立的 Rust/Vue 重写，没有复制 Momentum 的账户、Supabase、导入导出或部署实现，也不声称与理论作者或 Momentum 作者存在隶属、背书或官方关系。

## 更多文档

- [通用 OIDC 配置指引](docs/OIDC.md)
- [ZITADEL 配置指引](docs/ZITADEL.md)
- [架构与协议不变量](docs/ARCHITECTURE.md)

## 相关项目

- [Momentum](https://github.com/Kapozux/Momentum)
- [Bark](https://github.com/Finb/Bark)
- [ZITADEL](https://github.com/zitadel/zitadel)
- [Caddy](https://github.com/caddyserver/caddy)

## 许可

本项目采用 [MIT License](LICENSE) 开源。
