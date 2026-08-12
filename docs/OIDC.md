# 通用 OIDC 配置

本指引适用于 ZITADEL 以外、兼容 OpenID Connect 的身份提供方。不同产品的菜单名称可能不同，但需要配置的协议能力和地址相同。ZITADEL 用户可以直接阅读 [ZITADEL 配置指引](ZITADEL.md)。

以下示例假设：

- Lumeno 地址为 `https://focus.example.com/`；
- OIDC issuer 为 `https://id.example.com`。

请替换为真实地址。生产环境中的 Lumeno 和身份提供方都必须使用 HTTPS。

## 提供方要求

身份提供方必须提供标准 discovery 文档，并支持：

- Authorization Code Flow；
- 机密 Web 客户端和 Client Secret；
- Token Endpoint 的 `client_secret_basic` 认证；
- S256 PKCE；
- `openid profile email` scopes；
- 包含 `sub` 的签名 ID Token，以及可用于验证签名的 JWKS。

Lumeno 不调用 UserInfo Endpoint。若希望界面显示姓名和邮箱，提供方应把 `name` 或 `preferred_username`，以及 `email` 放入 ID Token。缺少这些可选 claims 不会阻止登录，但界面可能显示默认名称且不显示邮箱。

## 创建客户端

在身份提供方中新建一个机密 Web 应用，名称可设为 `Lumeno`，并使用 Authorization Code Flow。不要选择 SPA、Native、Implicit、Device Code、`client_secret_post` 或 Private Key JWT。

精确登记以下地址：

| 配置项 | 地址 | 是否必需 |
| --- | --- | --- |
| Redirect URI | `https://focus.example.com/api/v1/auth/callback` | 必需 |
| Post Logout Redirect URI | `https://focus.example.com/` | 提供方支持 RP 发起登出时配置 |
| Back-Channel Logout URI | `https://focus.example.com/api/v1/auth/backchannel-logout` | 提供方支持 Back-Channel Logout 时配置 |

协议、域名、端口、路径和末尾斜杠必须完全一致。Back-Channel Logout URI 需要能被身份提供方的服务器通过 HTTPS 访问。

创建后安全保存 Client ID 和 Client Secret。不要把 Client Secret 写进仓库、浏览器代码或日志。

## 填写环境变量

先读取 discovery 文档：

```bash
curl --fail --silent --show-error \
  https://id.example.com/.well-known/openid-configuration
```

确认响应中至少包含 `issuer`、`authorization_endpoint`、`token_endpoint` 和 `jwks_uri`。把响应中的 `issuer` 原样写入 `deploy/.env`：

```dotenv
LUMENO_DOMAIN=focus.example.com
LUMENO_PUBLIC_URL=https://focus.example.com/
OIDC_ISSUER=https://id.example.com
OIDC_CLIENT_ID=replace-with-client-id
OIDC_CLIENT_SECRET=replace-with-client-secret
OIDC_TRUSTED_AUDIENCES=
```

`LUMENO_PUBLIC_URL` 必须是带末尾 `/`、不含额外路径的站点根地址。修改 OIDC 配置后需要重启 Lumeno。

`OIDC_TRUSTED_AUDIENCES` 留空时，Lumeno 接受已验证 issuer 返回的 audience 值；如果 `aud` 不含当前 Client ID 或包含多个值，ID Token 必须同时包含与 Client ID 相同的 `azp`。这适用于使用项目 ID 作为 audience 的提供方。

如需严格限制非 Client ID audience，可将确认过的值以逗号分隔写入 `OIDC_TRUSTED_AUDIENCES`。配置后，ID Token 中的每个 `aud` 都必须是当前 Client ID 或白名单值，`azp` 客户端绑定规则仍然生效。

## 可选登出能力

如果 discovery 文档包含 `end_session_endpoint`，Lumeno 退出本地会话后会跳转到该地址，并携带 `client_id` 和 `post_logout_redirect_uri`。提供方不公布此端点时，Lumeno 仍会正常清除本地会话并返回首页。

如果提供方支持 OpenID Connect Back-Channel Logout，可登记上面的 Back-Channel Logout URI。Lumeno 接受包含标准 `logout_token` 表单字段的 POST 请求，并校验签名、issuer、audience、有效期、事件声明以及 `sid` 或 `sub`。登记 URI 后应重新登录，以便新的会话获得可用于关联登出的 `sid`。

## 验证与排错

1. 打开 Lumeno 首页并完成登录。
2. 确认回调后能看到预期的姓名或邮箱。
3. 从 Lumeno 退出，确认本地会话被清除；若提供方支持 RP 发起登出，再确认浏览器完成提供方退出并返回 Lumeno。
4. 若配置了 Back-Channel Logout，从提供方结束同一 SSO 会话，确认 Lumeno 会话随之失效。

常见问题：

- `redirect_uri` 被拒绝：逐项核对协议、域名、端口、路径和末尾斜杠。
- `unauthorized_client` 或换取 token 失败：确认客户端是机密 Web 应用，并启用了 `client_secret_basic`、Authorization Code Flow 和 S256 PKCE。
- 身份提供方未返回 ID token：确认请求包含 `openid` scope，且提供方按 OIDC 而不是纯 OAuth 2.0 配置客户端。
- ID Token 校验失败：确认 issuer 与 discovery 响应完全一致、主机时间准确、签名密钥可从 `jwks_uri` 获取。
- 姓名或邮箱为空：让提供方把 `name`、`preferred_username` 或 `email` 放入 ID Token；Lumeno 不读取 UserInfo Endpoint。
- `ID token 包含未受信任的 audience`：当前启用了严格 audience 白名单；确认该值后加入 `OIDC_TRUSTED_AUDIENCES`，或留空以使用 issuer + `azp` 客户端绑定模式。
- 后通道登出无效：确认 URI 可由提供方服务器访问、ID Token 含 `sid`，并在登记 URI 后重新登录。

协议参考：[OpenID Connect Discovery](https://openid.net/specs/openid-connect-discovery-1_0.html)、[OpenID Connect Core](https://openid.net/specs/openid-connect-core-1_0.html) 和 [OpenID Connect Back-Channel Logout](https://openid.net/specs/openid-connect-backchannel-1_0.html)。
