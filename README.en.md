<p align="center">
  <img src="web/public/icon.png" width="144" alt="Lumeno app icon">
</p>

# Lumeno

<p align="center">
  <a href="README.md">简体中文</a> · <strong>English</strong>
</p>

Lumeno is a self-management web application. It turns intended work into explicit starting conditions, actions, and time limits, then uses unpausable focus sessions, streaks, and failure reviews to reduce hesitation at the moment of action.

Theory source: this project is inspired by the method described by Edmond in the Zhihu answer [“How can I improve my self-control?”](https://www.zhihu.com/question/19888447/answer/1930799480401293785).

Lumeno is designed for deployment on a personal server or home network. It works in a browser and as an installed PWA on desktop, tablet, and mobile devices.

> Lumeno is a tool for behavioral tracking and reflection. It does not provide medical diagnosis or treatment. If problems with attention, mood, sleep, or impulse control significantly affect your life, seek help from a qualified professional.

> [!WARNING]
> This project contains AI-generated or AI-assisted code. It may include errors, security vulnerabilities, or unhandled edge cases. Review the code and test it thoroughly before using, deploying, or modifying it, especially before any production use.

## Features

- Contract chains: define tasks with a starting condition, required action, time limit, and booking signal.
- Strict focus: once started, time continues without a pause option; an early stop or interruption requires a judgment.
- Task groups: arrange multiple contracts into an ordered, nestable workflow.
- Recursive policies: trace backward from a failed outcome, then trial, revise, and deploy reusable interventions from a policy library.
- Policy groups: express tolerance through target and minimum active-member counts, with threshold-aware downstream rollback.
- Streaks and records: view total completions, failures, fulfillment rate, and an append-only protocol timeline.
- Offline continuity: synchronize in-progress focus facts in order after connectivity returns.
- External notifications: use Bark or a custom webhook, with optional Bark AES-256-GCM content encryption.
- Automatic time zone: calculate daily pattern-activation boundaries from the browser's current time zone.
- Responsive PWA: use desktop and mobile layouts, light or dark themes, and home-screen installation.
- OIDC sign-in: accounts and passwords remain managed by an existing identity provider.

## Quick Start

The default Lumeno deployment includes the application, PostgreSQL, and Caddy with HTTPS. Before starting, you need:

- A server with Git, OpenSSL, and Docker Compose installed;
- A domain name that resolves to the server, with ports 80 and 443 open;
- An existing ZITADEL or compatible OIDC identity provider.

### 1. Get the project and create the configuration

```bash
git clone https://github.com/LumenoProject/Lumeno.git
cd Lumeno
cp deploy/.env.example deploy/.env
```

Generate the database password and notification-configuration encryption key:

```bash
openssl rand -hex 32
openssl rand -base64 32 | tr '+/' '-_' | tr -d '='
```

### 2. Configure the environment

Edit `deploy/.env`:

| Variable | Purpose |
| --- | --- |
| `LUMENO_DOMAIN` | Domain used by Lumeno, such as `focus.example.com` |
| `LUMENO_PUBLIC_URL` | Complete site root URL, such as `https://focus.example.com/` |
| `OIDC_ISSUER` | Identity provider issuer |
| `OIDC_CLIENT_ID` | OIDC Client ID |
| `OIDC_CLIENT_SECRET` | OIDC Client Secret |
| `POSTGRES_PASSWORD` | Strong random password for the bundled PostgreSQL service |
| `NOTIFICATION_ENCRYPTION_KEY` | Server-side master key used to encrypt saved notification configuration |
| `NOTIFICATION_ENDPOINT_ALLOWLIST` | Allowlist of HTTPS origins permitted to receive notifications |

Store the two generated values in `POSTGRES_PASSWORD` and `NOTIFICATION_ENCRYPTION_KEY`, respectively. Set `LUMENO_DOMAIN` to the domain name only, and set `LUMENO_PUBLIC_URL` to the complete HTTPS URL with a trailing `/`.

Next, create a Lumeno web application in your identity provider and register its sign-in, post-logout, and back-channel logout URLs. See the [generic OIDC guide](docs/OIDC.md), or the dedicated [ZITADEL setup guide](docs/ZITADEL.md).

### 3. Start and verify the services

```bash
docker compose --env-file deploy/.env -f deploy/docker-compose.yml pull
docker compose --env-file deploy/.env -f deploy/docker-compose.yml up -d
docker compose --env-file deploy/.env -f deploy/docker-compose.yml ps
```

When every service is running or healthy, open `LUMENO_PUBLIC_URL` in a browser to sign in. Caddy obtains the HTTPS certificate automatically, so the first startup may take a short while.

## Using Lumeno

### 1. Create your first contract

Open “Contract Chains” and select “New Contract,” then enter:

- A name and description;
- A clearly recognizable starting condition;
- The action that must follow once the condition occurs;
- The required duration;
- An optional booking signal, delay, and 90-second start window.

When a task contains several ordered steps, create a task group first and add leaf tasks to it.

### 2. Start focusing from Today

The Today page shows actionable contracts, total completions, failures, and fulfillment rate. Totals are derived from append-only protocol events, so archiving a contract or creating a revision does not erase them. Starting an appointment on time updates only its booking streak and is not counted again as a completion; the subsequent focus session counts only after it is actually completed. Once a contract starts, its countdown continues through page refreshes, navigation, device sleep, and connectivity changes.

When the countdown ends, you can complete the focus session. If you interrupt or stop early, Lumeno asks whether the event is a failure or matches an existing or newly created exception rule.

Starting a focus session or booking immediately opens a full-screen countdown. You may cancel without consequence during the first 10 seconds, although elapsed time is still measured from the original click. After that window closes, the commitment becomes active and cannot be paused. When a booking reaches its scheduled time, a 90-second start window opens. Starting within that window increases the booking streak; missing it or terminating the booking opens a failure-or-exception judgment. You may return to the countdown before confirming termination. After judgment, the booking state is released so another focus session or booking can begin. If no judgment is made within five minutes, the server records a booking failure and resets the booking streak.

Archiving a contract does not remove cumulative records from the home page. Open “Contract Chains → View Archive” to see manually archived contracts and revisions preserved before changes to core commitments.

### 3. Turn a failure into the next intervention

Open “Patterns,” describe the unacceptable outcome, trace the sequence of decisions backward, and write a policy that can be recognized and performed next time. A policy first lives in the library. It can be deployed only after its current revision succeeds in a full trial on the same local date. At most one policy deployment is added per local day, with the server calculating the date from the account time zone.

Policy identity, immutable revisions, and tree deployments are separate. Editing after a trial or deployment creates a revision while existing deployments keep their original meaning. When an ordinary deployment fails, it and its dependent descendants are extinguished, while the policy remains in the library for a later trial and deployment. Cumulative active days are complete 24-hour periods derived from actual active seconds across deployments; they are not a habit-strength score.

Policy groups keep members separate from downstream dependencies. A group can support downstream deployments after reaching its minimum active-member count. A member failure that leaves the threshold intact extinguishes only that member. Falling below the threshold degrades the group and extinguishes downstream deployments while preserving other members. Adding enough members makes the group effective again, but previous downstream deployments do not revive automatically. An upstream failure extinguishes the group, its members, and its downstream tree.

### 4. Review protocol records

The Records page shows starts, completions, failures, and judgments in chronological order. Later edits cannot overwrite these records, so they reflect what actually happened rather than only retrospective impressions.

## External Notifications

Before notifications can be used, the deployer must add each permitted HTTPS origin to `NOTIFICATION_ENDPOINT_ALLOWLIST` in `deploy/.env`. Public, private-network, and Bark addresses that are not listed are rejected.

Under “Settings → Notifications,” choose a delivery method:

- Bark: paste the complete push URL shown by Bark; optional content encryption uses the same 32-character key in Lumeno and the Bark client.
- Custom webhook: provide an HTTPS URL and, optionally, a Bearer token.

When a booking, focus session, or task-group deadline arrives, the backend delivers the notification directly. The browser does not need to remain open or signed in. Saved notification addresses and secrets are not returned to the browser; enter the complete configuration again when changing it. Use “Test Notification” to verify connectivity.

### Notification target allowlist

The allowlist is comma-separated and must specify the exact scheme, host, and port. The official Bark service and trusted self-hosted Bark or webhook services can be configured as follows:

```env
NOTIFICATION_ENDPOINT_ALLOWLIST=https://api.day.app,https://192.168.1.20:8443,https://bark.internal.example
```

Do not put a Bark device key, webhook path, query parameter, or other secret in the allowlist. Enter the complete push URL only in the Settings page, for example:

```text
https://192.168.1.20:8443/device-key
```

The allowlisted origin must match the complete address exactly. For example, `https://example.com` does not allow `https://example.com:8443`. Internal services must use an HTTPS certificate trusted by the application runtime. If the deployment uses an outbound proxy, that proxy must also be able to reach the target.

## PWA and Mobile

Open Lumeno in a browser with PWA support, then use the browser menu to install it on a desktop or mobile home screen. The mobile navigation provides six entries: Today, Contract Chains, Patterns, Records, Guide, and Settings.

The browser time zone is synchronized after sign-in and determines the local date boundary for same-day trials and the once-per-day deployment rule. Reload the application after the device time zone changes.

## Upgrades and Backups

For production deployments, pin `LUMENO_IMAGE` in `deploy/.env` to a released version tag or image digest. Pull the new image and restart the services to upgrade:

```bash
docker compose --env-file deploy/.env -f deploy/docker-compose.yml pull
docker compose --env-file deploy/.env -f deploy/docker-compose.yml up -d
```

On first startup, the application automatically initializes an empty database with the schema required by the current version. The phase-two RSIP model does not include an in-place upgrade or backfill path for the earlier `rsip_nodes` schema; do not apply this initial migration directly to an existing earlier database. Users, contracts, records, and notification configuration are stored in PostgreSQL. Back up the `postgres-data` volume or external database regularly, and verify that backups can be restored.

To use an external PostgreSQL database, set `DATABASE_URL` in `deploy/.env`, then start only the application and ingress services:

```bash
docker compose --env-file deploy/.env -f deploy/docker-compose.yml up -d --no-deps app caddy
```

## Theory and Third-Party Attribution

This project is inspired by the method described by Edmond in the Zhihu answer [“How can I improve my self-control?”](https://www.zhihu.com/question/19888447/answer/1930799480401293785). The product semantics of CTDP and RSIP derive from the method described publicly in that answer.

The implementation design also references Ken Xiao's open-source project [Momentum](https://github.com/Kapozux/Momentum). Momentum copyright © 2025 Ken Xiao; the original project is released under the MIT License.

Lumeno is an independent Rust/Vue rewrite. It does not copy Momentum's account, Supabase, import/export, or deployment implementation, and it does not claim affiliation with, endorsement by, or official status from the theory's author or Momentum's author.

## Additional Documentation

- [Generic OIDC setup guide](docs/OIDC.md)
- [ZITADEL setup guide](docs/ZITADEL.md)
- [Architecture and protocol invariants](docs/ARCHITECTURE.md)

## Related Projects

- [Momentum](https://github.com/Kapozux/Momentum)
- [Bark](https://github.com/Finb/Bark)
- [ZITADEL](https://github.com/zitadel/zitadel)
- [Caddy](https://github.com/caddyserver/caddy)

## License

This project is open source under the [MIT License](LICENSE).
