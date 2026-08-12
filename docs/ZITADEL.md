# ZITADEL 配置

本指引只说明如何把 Lumeno 接入一个已经可用的 ZITADEL 实例，不涉及 ZITADEL
本身的部署。使用其他身份提供方时，请阅读[通用 OIDC 配置指引](OIDC.md)。示例假设：

- Lumeno 地址为 `https://focus.example.com/`；
- ZITADEL issuer 为 `https://id.example.com`。

请把它们替换为真实地址。生产环境中的 Lumeno 和 ZITADEL 都必须使用 HTTPS。

## 创建应用

1. 登录 ZITADEL Management Console，进入用于 Lumeno 的 Project。不要使用或修改
   ZITADEL 自带的默认项目。
2. 在 Applications 中新建应用，名称可设为 `Lumeno`，Application Type 选择
   `Web`。
3. Authentication Method 选择 `Code` / `Basic`（`client_secret_basic`）。Lumeno
   是保存 client secret 的服务端 Web 应用，并会在 Authorization Code Flow 上额外使用
   S256 PKCE。不要选择 `Post`、`User Agent`、`Native` 或 Private Key JWT；当前实现不使用
   这些客户端认证方式。
4. 关闭生产应用的 Development Mode，并精确登记以下地址：

   | 配置项 | 地址 |
   | --- | --- |
   | Redirect URI | `https://focus.example.com/api/v1/auth/callback` |
   | Post Logout Redirect URI | `https://focus.example.com/` |
   | Back-Channel Logout URI | `https://focus.example.com/api/v1/auth/backchannel-logout` |

   ZITADEL 会精确匹配 Redirect URI，根地址末尾的 `/` 也不能省略。Back-Channel
   Logout URI 必须能被 ZITADEL 服务端通过公网 HTTPS 访问；设置后，新的登录会话才会
   注册后通道登出通知。
5. 创建应用并立即安全保存 Client ID 和 Client Secret。不要把 secret 写进仓库、日志或
   客户端代码。

## Token 设置

在应用的 Token Settings 中启用 **User Info inside ID Token**。Lumeno 请求
`openid profile email`，并直接从经过验证的 ID Token 读取姓名、用户名和邮箱；ZITADEL
默认不会在签发 access token 时把 `profile` 和 `email` 声明放进 ID Token，因此这个开关
是必需的。

Lumeno 当前不读取 ZITADEL 项目角色，无需打开 role assertion。所有浏览器 API 都与
Lumeno 同源，也无需为此添加 Additional Origins/CORS 配置。

## 填写环境变量

先读取 discovery 文档中的 `issuer`：

```bash
curl --fail --silent --show-error \
  https://id.example.com/.well-known/openid-configuration
```

把响应里的 `issuer` 原样用作 `OIDC_ISSUER`，不要自行添加路径或末尾斜杠。然后填写
最终部署使用 `deploy/.env`；源码本地运行时把相同变量导入当前环境，并显式设置本地可访问的 `DATABASE_URL`：

```dotenv
LUMENO_DOMAIN=focus.example.com
LUMENO_PUBLIC_URL=https://focus.example.com/
OIDC_ISSUER=https://id.example.com
OIDC_CLIENT_ID=replace-with-zitadel-client-id
OIDC_CLIENT_SECRET=replace-with-zitadel-client-secret
OIDC_TRUSTED_AUDIENCES=
```

`LUMENO_PUBLIC_URL` 必须是带末尾 `/`、不含额外路径的站点根地址。修改 ZITADEL 地址、
Client ID 或认证方式后，需要重启 Lumeno。

ZITADEL 的 ID Token 可能使用项目 ID，或同时返回项目内多个 Client ID。保持
`OIDC_TRUSTED_AUDIENCES` 为空时，Lumeno 不限制这些由已验证 issuer 签发的 audience 值，
但只要 `aud` 不含当前 Client ID 或包含多个值，就必须有与 `OIDC_CLIENT_ID` 完全相同的
`azp`，从而把令牌绑定到当前客户端。

如需进一步限制允许出现的非 Client ID audience，可将确认过的项目 ID 或应用 ID 加入
`OIDC_TRUSTED_AUDIENCES`。多个值使用逗号分隔，例如：

```dotenv
OIDC_TRUSTED_AUDIENCES=385667293859801872
```

不要把 `OIDC_CLIENT_ID` 改成项目 ID，也不要加入来源不明的 audience。无论该变量是否留空，
Lumeno 都会校验签名、issuer、有效期、nonce 和客户端绑定。

## 验证

1. 打开 `https://focus.example.com/` 并登录。
2. 确认回调后能看到 ZITADEL 用户的姓名或邮箱，而不是默认名称。
3. 从 Lumeno 退出，确认浏览器进入 ZITADEL 的 end-session 流程并返回 Lumeno。
4. 重新登录以创建已登记 Back-Channel Logout URI 的新会话，再从 ZITADEL 或另一个应用
   结束同一 SSO 会话，确认 Lumeno 的本地会话随之失效。

常见问题：

- `redirect_uri` 被拒绝：核对协议、域名、端口、路径和末尾斜杠是否完全一致。
- code exchange 返回 `unauthorized_client`：核对 Web 应用的 `Code` / `Basic` 认证方式、
  Client ID 和 Client Secret。
- 用户名或邮箱为空：确认 **User Info inside ID Token** 已启用，并让用户重新登录。
- discovery 或 token 校验失败：确认 `OIDC_ISSUER` 与 discovery 响应中的 `issuer` 完全一致，
  ZITADEL 的 HTTPS 证书有效，且 Lumeno 主机时间已同步。
- `ID token 包含未受信任的 audience`：当前启用了严格 audience 白名单；确认该值属于当前
  ZITADEL 项目后加入 `OIDC_TRUSTED_AUDIENCES`，或留空以使用 issuer + `azp` 客户端绑定模式。
- 后通道登出无效：确认公网可访问 Back-Channel Logout URI，并在登记 URI 后重新登录。

参考 ZITADEL 官方文档：[Applications](https://zitadel.com/docs/guides/manage/console/applications-overview)、
[Authorization Code Flow + PKCE](https://zitadel.com/docs/guides/integrate/login/oidc/login-users)、
[OIDC endpoints](https://zitadel.com/docs/apis/openidoauth/endpoints) 和
[Back-Channel Logout](https://zitadel.com/docs/guides/integrate/back-channel-logout)。
